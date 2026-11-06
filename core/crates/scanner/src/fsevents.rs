//! Which folders changed since an earlier moment, from macOS's file event
//! history (FSEvents). Read only: this asks the system's event log, and
//! never opens, changes or watches a file itself. Used for faster rescans
//! (F12, see [`crate::rescan`]).
//!
//! How it is used:
//!
//! 1. Just before a scan starts walking, [`current_event_id`] notes where
//!    the system's event log is. Anything that changes during or after the
//!    walk has a later id.
//! 2. For the next scan, [`changes_since`] replays the log from that id
//!    for the scanned folder only, and returns the folders it names.
//!
//! It says "do a full scan" ([`NoHistory`]) whenever the log cannot be
//! trusted to be complete: events were dropped, ids wrapped, the folder
//! itself moved, a volume was mounted or unmounted, the replay did not
//! finish in time, or (on any system but macOS) there is no such log.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::Changes;

/// How long [`changes_since`] waits for the replay to finish.
pub const REPLAY_TIMEOUT: Duration = Duration::from_secs(10);

/// Why there is no trustworthy list of changes. Do a full scan instead.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("no usable file event history: {0}")]
pub struct NoHistory(pub &'static str);

/// The system's latest file event id, or `None` where there is no event
/// log (any system but macOS).
pub fn current_event_id() -> Option<u64> {
    imp::current_event_id()
}

/// The folders under `root` the system's event log says changed after
/// event `since`. Waits up to `timeout` for the replay.
pub fn changes_since(root: &Path, since: u64, timeout: Duration) -> Result<Changes, NoHistory> {
    imp::changes_since(root, since, timeout)
}

// Only the macOS code (and the tests) use this on other systems.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// What one replayed event means for the rescan.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    /// Something directly in this folder changed.
    Folder(PathBuf),
    /// Anything below this folder may have changed.
    Subtree(PathBuf),
    /// The log can't be trusted: full scan.
    Untrusted(&'static str),
    /// Nothing to do (for example the replay-finished marker).
    Ignore,
}

// Only the macOS code (and the tests) use this on other systems.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// Event flags, from `FSEvents.h`.
mod flags {
    pub const MUST_SCAN_SUB_DIRS: u32 = 0x0000_0001;
    pub const USER_DROPPED: u32 = 0x0000_0002;
    pub const KERNEL_DROPPED: u32 = 0x0000_0004;
    pub const EVENT_IDS_WRAPPED: u32 = 0x0000_0008;
    pub const HISTORY_DONE: u32 = 0x0000_0010;
    pub const ROOT_CHANGED: u32 = 0x0000_0020;
    pub const MOUNT: u32 = 0x0000_0040;
    pub const UNMOUNT: u32 = 0x0000_0080;
}

// Only the macOS code (and the tests) use this on other systems.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// Reads one event's flags. Pure, so it is tested on every system.
fn classify(path: &Path, event_flags: u32) -> Event {
    use flags::*;
    if event_flags & (USER_DROPPED | KERNEL_DROPPED) != 0 {
        return Event::Untrusted("file events were dropped");
    }
    if event_flags & EVENT_IDS_WRAPPED != 0 {
        return Event::Untrusted("file event ids wrapped");
    }
    if event_flags & ROOT_CHANGED != 0 {
        return Event::Untrusted("the scanned folder moved");
    }
    if event_flags & (MOUNT | UNMOUNT) != 0 {
        return Event::Untrusted("a volume was mounted or unmounted");
    }
    if event_flags & HISTORY_DONE != 0 {
        return Event::Ignore;
    }
    if event_flags & MUST_SCAN_SUB_DIRS != 0 {
        return Event::Subtree(path.to_path_buf());
    }
    Event::Folder(path.to_path_buf())
}

// Only the macOS code (and the tests) use this on other systems.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// Collects classified events into [`Changes`]. A path outside `root` makes
/// the whole list untrusted. Pure, so it is tested on every system.
fn collect(root: &Path, events: impl IntoIterator<Item = Event>) -> Result<Changes, NoHistory> {
    let mut changes = Changes::default();
    for event in events {
        match event {
            Event::Folder(p) => changes.folders.push(inside(root, p)?),
            Event::Subtree(p) => changes.subtrees.push(inside(root, p)?),
            Event::Untrusted(why) => return Err(NoHistory(why)),
            Event::Ignore => {}
        }
    }
    changes.folders.sort();
    changes.folders.dedup();
    changes.subtrees.sort();
    changes.subtrees.dedup();
    Ok(changes)
}

// Only the macOS code (and the tests) use this on other systems.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// `path` if it is `root` or inside it. The system can name the home
/// folder through `/System/Volumes/Data`, so that prefix is allowed too.
fn inside(root: &Path, path: PathBuf) -> Result<PathBuf, NoHistory> {
    if path.starts_with(root) {
        return Ok(path);
    }
    if let Ok(rest) = path.strip_prefix("/System/Volumes/Data") {
        let rebuilt = Path::new("/").join(rest);
        if rebuilt.starts_with(root) {
            return Ok(rebuilt);
        }
    }
    Err(NoHistory("a file event is outside the scanned folder"))
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;

    pub fn current_event_id() -> Option<u64> {
        None
    }

    pub fn changes_since(
        _root: &Path,
        _since: u64,
        _timeout: Duration,
    ) -> Result<Changes, NoHistory> {
        Err(NoHistory("this system keeps no file event history"))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    //! The CoreServices calls. Each `unsafe` block says why it is sound.

    use super::*;
    use std::ffi::{c_char, c_void, CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Instant;

    type CFAllocatorRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFIndex = isize;
    type FSEventStreamRef = *mut c_void;
    type DispatchQueue = *mut c_void;

    #[repr(C)]
    struct FSEventStreamContext {
        version: CFIndex,
        info: *mut c_void,
        retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
        release: Option<extern "C" fn(*const c_void)>,
        copy_description: Option<extern "C" fn(*const c_void) -> CFStringRef>,
    }

    type Callback = extern "C" fn(
        stream: FSEventStreamRef,
        info: *mut c_void,
        count: usize,
        paths: *mut c_void,
        flags: *const u32,
        ids: *const u64,
    );

    /// `kFSEventStreamCreateFlagNoDefer | kFSEventStreamCreateFlagWatchRoot`.
    const CREATE_FLAGS: u32 = 0x2 | 0x4;
    /// Seconds the service may hold events before delivering them.
    const LATENCY: f64 = 0.05;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFTypeArrayCallBacks: c_void;
        fn CFArrayCreate(
            allocator: CFAllocatorRef,
            values: *const *const c_void,
            count: CFIndex,
            callbacks: *const c_void,
        ) -> CFArrayRef;
        fn CFStringCreateWithFileSystemRepresentation(
            allocator: CFAllocatorRef,
            buffer: *const c_char,
        ) -> CFStringRef;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn FSEventsGetCurrentEventId() -> u64;
        fn FSEventStreamCreate(
            allocator: CFAllocatorRef,
            callback: Callback,
            context: *const FSEventStreamContext,
            paths: CFArrayRef,
            since: u64,
            latency: f64,
            flags: u32,
        ) -> FSEventStreamRef;
        fn FSEventStreamSetDispatchQueue(stream: FSEventStreamRef, queue: DispatchQueue);
        fn FSEventStreamStart(stream: FSEventStreamRef) -> u8;
        fn FSEventStreamFlushSync(stream: FSEventStreamRef);
        fn FSEventStreamStop(stream: FSEventStreamRef);
        fn FSEventStreamInvalidate(stream: FSEventStreamRef);
        fn FSEventStreamRelease(stream: FSEventStreamRef);
    }

    extern "C" {
        fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> DispatchQueue;
        fn dispatch_release(object: *mut c_void);
    }

    /// What the callback collects, behind a lock.
    #[derive(Default)]
    struct Replay {
        events: Vec<Event>,
        done: bool,
    }

    type Shared = (Mutex<Replay>, Condvar);

    pub fn current_event_id() -> Option<u64> {
        // SAFETY: takes no arguments and only reads the service's counter.
        Some(unsafe { FSEventsGetCurrentEventId() })
    }

    pub fn changes_since(root: &Path, since: u64, timeout: Duration) -> Result<Changes, NoHistory> {
        let events = replay(root, since, timeout)?;
        collect(root, events)
    }

    /// Called by the service on its queue with a batch of events. `paths`
    /// is a C array of C strings (no `UseCFTypes` flag).
    extern "C" fn on_events(
        _stream: FSEventStreamRef,
        info: *mut c_void,
        count: usize,
        paths: *mut c_void,
        event_flags: *const u32,
        _ids: *const u64,
    ) {
        if info.is_null() || paths.is_null() || event_flags.is_null() {
            return;
        }
        // SAFETY: `info` is the `Arc<Shared>` pointer given in the context,
        // kept alive by the stream through `retain_info`/`release_info`.
        let shared = unsafe { &*(info as *const Shared) };
        // SAFETY: the service passes `count` paths and `count` flags.
        let (paths, flag_list) = unsafe {
            (
                std::slice::from_raw_parts(paths as *const *const c_char, count),
                std::slice::from_raw_parts(event_flags, count),
            )
        };
        let mut batch = Vec::with_capacity(count);
        let mut done = false;
        for (&p, &f) in paths.iter().zip(flag_list) {
            if f & flags::HISTORY_DONE != 0 {
                done = true;
            }
            if p.is_null() {
                batch.push(Event::Untrusted("a file event had no path"));
                continue;
            }
            // SAFETY: each path is a NUL-terminated string owned by the
            // service for the length of this call.
            let bytes = unsafe { CStr::from_ptr(p) }.to_bytes();
            let path = Path::new(std::ffi::OsStr::from_bytes(bytes));
            batch.push(classify(path, f));
        }
        let (lock, wake) = shared;
        let mut replay = lock.lock().unwrap_or_else(|e| e.into_inner());
        replay.events.extend(batch);
        replay.done |= done;
        wake.notify_all();
    }

    extern "C" fn retain_info(info: *const c_void) -> *const c_void {
        // SAFETY: `info` came from `Arc::into_raw` and is still alive.
        unsafe { Arc::increment_strong_count(info as *const Shared) };
        info
    }

    extern "C" fn release_info(info: *const c_void) {
        // SAFETY: balances a `retain_info` (or the first `into_raw`).
        unsafe { Arc::decrement_strong_count(info as *const Shared) };
    }

    /// Replays events for `root` after `since`, until the service says the
    /// history is done, then flushes anything still pending.
    fn replay(root: &Path, since: u64, timeout: Duration) -> Result<Vec<Event>, NoHistory> {
        let root_c = CString::new(root.as_os_str().as_bytes())
            .map_err(|_| NoHistory("the folder name holds a NUL byte"))?;
        let shared: Arc<Shared> = Arc::new((Mutex::new(Replay::default()), Condvar::new()));
        let info = Arc::into_raw(Arc::clone(&shared)) as *mut c_void;
        let context = FSEventStreamContext {
            version: 0,
            info,
            retain: Some(retain_info),
            release: Some(release_info),
            copy_description: None,
        };
        // SAFETY: plain CoreFoundation and CoreServices calls with valid
        // arguments. Every object made here is released below, on every
        // path; the stream retains `info` itself, so dropping our own
        // reference at the end is sound even if a late callback runs.
        let outcome = unsafe {
            let path =
                CFStringCreateWithFileSystemRepresentation(std::ptr::null(), root_c.as_ptr());
            if path.is_null() {
                release_info(info);
                return Err(NoHistory("the folder name could not be passed on"));
            }
            let values = [path];
            let array = CFArrayCreate(
                std::ptr::null(),
                values.as_ptr(),
                1,
                std::ptr::addr_of!(kCFTypeArrayCallBacks),
            );
            CFRelease(path);
            if array.is_null() {
                release_info(info);
                return Err(NoHistory("the folder list could not be made"));
            }
            let stream = FSEventStreamCreate(
                std::ptr::null(),
                on_events,
                &context,
                array,
                since,
                LATENCY,
                CREATE_FLAGS,
            );
            CFRelease(array);
            if stream.is_null() {
                release_info(info);
                return Err(NoHistory("the file event stream could not be made"));
            }
            let label = c"com.roomy.rescan-events";
            let queue = dispatch_queue_create(label.as_ptr(), std::ptr::null());
            FSEventStreamSetDispatchQueue(stream, queue);
            let started = FSEventStreamStart(stream) != 0;
            let outcome = if started {
                let finished = wait_for_history(&shared, timeout);
                if finished {
                    FSEventStreamFlushSync(stream);
                }
                FSEventStreamStop(stream);
                Some(finished)
            } else {
                None
            };
            FSEventStreamInvalidate(stream);
            FSEventStreamRelease(stream);
            dispatch_release(queue);
            release_info(info);
            outcome
        };
        match outcome {
            None => Err(NoHistory("the file event stream did not start")),
            Some(false) => Err(NoHistory("the file event history did not finish in time")),
            Some(true) => {
                let (lock, _) = &*shared;
                let mut replay = lock.lock().unwrap_or_else(|e| e.into_inner());
                Ok(std::mem::take(&mut replay.events))
            }
        }
    }

    /// Waits until the callback has seen the history-done marker.
    fn wait_for_history(shared: &Shared, timeout: Duration) -> bool {
        let (lock, wake) = shared;
        let deadline = Instant::now() + timeout;
        let mut replay = lock.lock().unwrap_or_else(|e| e.into_inner());
        while !replay.done {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            replay = match wake.wait_timeout(replay, deadline - now) {
                Ok((guard, _)) => guard,
                Err(e) => e.into_inner().0,
            };
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_that_lose_events_mean_a_full_scan() {
        let p = Path::new("/Users/sam/Library");
        for f in [
            flags::USER_DROPPED,
            flags::KERNEL_DROPPED,
            flags::EVENT_IDS_WRAPPED,
            flags::ROOT_CHANGED,
            flags::MOUNT,
            flags::UNMOUNT,
            flags::MUST_SCAN_SUB_DIRS | flags::KERNEL_DROPPED,
        ] {
            assert!(matches!(classify(p, f), Event::Untrusted(_)), "{f:#x}");
        }
        assert_eq!(classify(p, flags::HISTORY_DONE), Event::Ignore);
        assert_eq!(classify(p, 0), Event::Folder(p.to_path_buf()));
        assert_eq!(
            classify(p, flags::MUST_SCAN_SUB_DIRS),
            Event::Subtree(p.to_path_buf())
        );
    }

    #[test]
    fn events_are_collected_sorted_and_once() {
        let root = Path::new("/Users/sam");
        let got = collect(
            root,
            [
                Event::Folder("/Users/sam/b/".into()),
                Event::Folder("/Users/sam/a".into()),
                Event::Folder("/Users/sam/a".into()),
                Event::Ignore,
                Event::Subtree("/System/Volumes/Data/Users/sam/c".into()),
            ],
        )
        .unwrap();
        assert_eq!(
            got,
            Changes {
                folders: vec!["/Users/sam/a".into(), "/Users/sam/b".into()],
                subtrees: vec!["/Users/sam/c".into()],
            }
        );
    }

    #[test]
    fn an_event_outside_the_folder_or_an_untrusted_one_means_a_full_scan() {
        let root = Path::new("/Users/sam");
        assert!(collect(root, [Event::Folder("/Users/other".into())]).is_err());
        assert!(collect(root, [Event::Folder("/Users/samuel".into())]).is_err());
        assert!(collect(
            root,
            [Event::Folder("/Users/sam".into()), Event::Untrusted("x")]
        )
        .is_err());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn other_systems_have_no_history() {
        assert_eq!(current_event_id(), None);
        assert!(changes_since(Path::new("/"), 0, REPLAY_TIMEOUT).is_err());
    }
}

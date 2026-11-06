#!/bin/bash
# Mac storage survey v1 (read-only)
#
# What it does: measures how much space common categories use on this Mac
# (backups, caches, Xcode, Android, Docker, node_modules and so on) and prints
# a short report of sizes only. It never prints file names, folder names
# inside your files, file contents, your username or your computer's name.
# It never changes, moves or deletes anything.
#
# How to run (Terminal):
#   bash ~/Downloads/mac-storage-survey.sh
#
# macOS may ask whether Terminal can access Desktop, Documents, Downloads or
# other folders. "Allow" gives more complete numbers; "Don't Allow" is fine,
# the script will mark those items as "no access" and carry on.
# When it finishes, the report is copied to your clipboard: paste it back to
# whoever sent you this script.

set -u
export LC_ALL=C

H="$HOME"
REPORT=""

line() { REPORT="${REPORT}$1"$'\n'; }

to_gb() { awk -v k="$1" 'BEGIN { printf "%.1f GB", k / 1048576 }'; }

progress() { printf '  measuring %s...\n' "$1" >&2; }

# Size of one path, in KB. Prints "missing", "no access", or a number.
size_of() {
  local p="$1" out
  if [ ! -e "$p" ]; then echo "missing"; return; fi
  if [ ! -r "$p" ]; then echo "no access"; return; fi
  out=$(du -sk "$p" 2>/dev/null | awk '{print $1}')
  if [ -z "$out" ]; then echo "no access"; else echo "$out"; fi
}

row() {
  # row "Label" value_kb_or_status [note]
  local label="$1" v="$2" note="${3:-}" shown
  case "$v" in
    missing|"no access"|"not installed") shown="$v" ;;
    *) shown=$(to_gb "$v") ;;
  esac
  if [ -n "$note" ]; then
    line "$(printf '  %-40s %12s  %s' "$label" "$shown" "$note")"
  else
    line "$(printf '  %-40s %12s' "$label" "$shown")"
  fi
}

measure() {
  # measure "Label" path
  progress "$1"
  row "$1" "$(size_of "$2")"
}

# Sum the sizes of everything a find expression prints (NUL separated).
sum_found() { xargs -0 du -sk 2>/dev/null | awk '{s += $1} END {print s + 0}'; }
count_found() { tr -cd '\0' | wc -c | tr -d ' '; }

# Folders to skip when hunting for project folders such as node_modules.
find_projects() {
  # find_projects <name> [path-suffix-filter]
  find "$H" \
    \( -path "$H/Library" -o -path "$H/.Trash" -o -path "$H/Pictures" \
       -o -path "$H/Music" -o -path "$H/Movies" -o -path "$H/Applications" \
       -o -path "$H/.cache" -o -path "$H/.npm" -o -path "$H/.gradle" \
       -o -path "$H/.cargo" -o -path "$H/.rustup" -o -path "$H/.android" \) -prune \
    -o -type d -name "$1" -prune -print0 2>/dev/null
}

echo "Mac storage survey: read-only, sizes only. This can take a few minutes." >&2
echo >&2

# Profile
printf 'Which describes you best? [1] Everyday user  [2] Creator (video/music/design)  [3] Developer : ' >&2
read -r choice || choice=""
case "$choice" in
  2) profile="creator" ;;
  3) profile="developer" ;;
  *) profile="everyday" ;;
esac

line "=== Mac storage survey v1 ==="
line "Profile: $profile"

os=$(sw_vers -productVersion 2>/dev/null || echo "unknown")
arch=$(uname -m)
ram_bytes=$(sysctl -n hw.memsize 2>/dev/null || echo 0)
ram_gb=$(awk -v b="$ram_bytes" 'BEGIN { printf "%.0f", b / 1073741824 }')
line "macOS $os | $arch | ${ram_gb} GB RAM"

data_vol="/System/Volumes/Data"
[ -d "$data_vol" ] || data_vol="/"
read -r total used avail <<EOF
$(df -k "$data_vol" | awk 'NR == 2 {print $2, $3, $4}')
EOF
line "Disk: $(to_gb "$total") total, $(to_gb "$avail") free"
line ""

# ---------------------------------------------------------------- Everyday
line "[Everyday]"
measure "iPhone/iPad backups"            "$H/Library/Application Support/MobileSync/Backup"
if [ -r "$H/Library/Application Support/MobileSync/Backup" ]; then
  n=$(find "$H/Library/Application Support/MobileSync/Backup" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' ')
  line "$(printf '  %-40s %12s' "  number of device backups" "$n")"
fi
measure "Messages attachments"           "$H/Library/Messages/Attachments"
measure "Mail data"                      "$H/Library/Mail"
measure "Downloads folder (total)"       "$H/Downloads"
progress "installers in Downloads"
if [ -r "$H/Downloads" ]; then
  kb=$(find "$H/Downloads" -maxdepth 4 -type f \
        \( -iname '*.dmg' -o -iname '*.pkg' -o -iname '*.ipsw' -o -iname '*.zip' -o -iname '*.xip' \) \
        -print0 2>/dev/null | sum_found)
  row "  installers and archives in Downloads" "$kb"
elif [ ! -e "$H/Downloads" ]; then
  row "  installers and archives in Downloads" "missing"
else
  row "  installers and archives in Downloads" "no access"
fi
progress "Photos libraries"
if [ -r "$H/Pictures" ]; then
  kb=$(find "$H/Pictures" -maxdepth 2 -name '*.photoslibrary' -print0 2>/dev/null | sum_found)
  row "Photos library (total)" "$kb"
elif [ ! -e "$H/Pictures" ]; then
  row "Photos library (total)" "missing"
else
  row "Photos library (total)" "no access"
fi
measure "Music folder"                   "$H/Music"
measure "Trash"                          "$H/.Trash"
measure "User caches (~/Library/Caches)" "$H/Library/Caches"
measure "App containers"                 "$H/Library/Containers"
measure "Group containers"               "$H/Library/Group Containers"
measure "Application Support"            "$H/Library/Application Support"
measure "Logs"                           "$H/Library/Logs"
measure "iOS software updates"           "$H/Library/iTunes/iPhone Software Updates"
line ""

# ---------------------------------------------------------------- Creators
line "[Creators]"
progress "Final Cut libraries"
if [ -r "$H/Movies" ]; then
  kb=$(find "$H/Movies" -maxdepth 3 -name '*.fcpbundle' -prune -print0 2>/dev/null | sum_found)
  row "Final Cut libraries (total)" "$kb"
elif [ ! -e "$H/Movies" ]; then
  row "Final Cut libraries (total)" "missing"
else
  row "Final Cut libraries (total)" "no access"
fi
measure "Logic/GarageBand sound library" "/Library/Application Support/Logic"
measure "GarageBand content"             "/Library/Application Support/GarageBand"
measure "Adobe media cache"              "$H/Library/Application Support/Adobe/Common/Media Cache Files"
measure "Adobe media cache database"     "$H/Library/Application Support/Adobe/Common/Media Cache"
line ""

# ---------------------------------------------------------------- Developers
line "[Developer: Xcode]"
if [ -d "/Applications/Xcode.app" ] || [ -d "$H/Library/Developer/Xcode" ]; then
  measure "DerivedData"                  "$H/Library/Developer/Xcode/DerivedData"
  measure "Archives"                     "$H/Library/Developer/Xcode/Archives"
  measure "iOS DeviceSupport"            "$H/Library/Developer/Xcode/iOS DeviceSupport"
  measure "watchOS DeviceSupport"        "$H/Library/Developer/Xcode/watchOS DeviceSupport"
  measure "Simulator devices"            "$H/Library/Developer/CoreSimulator/Devices"
  measure "Simulator caches"             "$H/Library/Developer/CoreSimulator/Caches"
  measure "Simulator runtimes (system)"  "/Library/Developer/CoreSimulator/Volumes"
  measure "Xcode app"                    "/Applications/Xcode.app"
  measure "CocoaPods cache"              "$H/Library/Caches/CocoaPods"
else
  row "Xcode" "not installed"
fi
line ""

line "[Developer: Android]"
if [ -d "$H/Library/Android/sdk" ] || [ -d "$H/.android" ] || [ -d "$H/.gradle" ]; then
  measure "Android SDK (total)"          "$H/Library/Android/sdk"
  measure "  system images"              "$H/Library/Android/sdk/system-images"
  measure "  NDK"                        "$H/Library/Android/sdk/ndk"
  measure "  build-tools"                "$H/Library/Android/sdk/build-tools"
  measure "Emulators (AVDs)"             "$H/.android/avd"
  measure "Gradle (total)"               "$H/.gradle"
  measure "  Gradle caches"              "$H/.gradle/caches"
  measure "  Gradle wrapper versions"    "$H/.gradle/wrapper/dists"
else
  row "Android tooling" "not installed"
fi
line ""

line "[Developer: JavaScript and React Native]"
progress "node_modules folders (this is the slow part)"
nm_count=$(find_projects node_modules | count_found)
nm_kb=$(find_projects node_modules | sum_found)
row "node_modules (all projects)" "$nm_kb" "($nm_count projects)"
progress "iOS Pods folders in projects"
pods_kb=$(find "$H" \( -path "$H/Library" -o -path "$H/.Trash" \) -prune \
          -o -type d -path '*/ios/Pods' -prune -print0 2>/dev/null | sum_found)
row "ios/Pods in projects" "$pods_kb"
measure "npm cache"                      "$H/.npm"
measure "pnpm store"                     "$H/Library/pnpm"
measure "Yarn cache"                     "$H/Library/Caches/Yarn"
measure "Bun cache"                      "$H/.bun/install/cache"
measure "Expo cache"                     "$H/.expo"
measure "Temp folder (Metro and others)" "${TMPDIR:-/tmp}"
line ""

line "[Developer: Docker]"
if [ -d "$H/Library/Containers/com.docker.docker" ]; then
  measure "Docker Desktop data"          "$H/Library/Containers/com.docker.docker/Data"
else
  row "Docker Desktop" "not installed"
fi
measure "OrbStack data"                  "$H/.orbstack"
measure "Colima data"                    "$H/.colima"
line ""

line "[Developer: other]"
measure "Homebrew cache"                 "$H/Library/Caches/Homebrew"
measure "pip cache"                      "$H/Library/Caches/pip"
measure "uv cache"                       "$H/.cache/uv"
measure "Rust toolchains (~/.rustup)"    "$H/.rustup"
measure "Cargo registry"                 "$H/.cargo/registry"
progress "Rust target folders"
tgt_kb=$(find_projects target | sum_found)
row "Rust/other target folders" "$tgt_kb"
measure "Ollama models"                  "$H/.ollama/models"
measure "LM Studio models"               "$H/.lmstudio/models"
measure "Hugging Face cache"             "$H/.cache/huggingface"
measure "Claude Code data"               "$H/.claude"
measure "VS Code data"                   "$H/Library/Application Support/Code"
measure "Cursor data"                    "$H/Library/Application Support/Cursor"
line ""
line "Generated $(date '+%Y-%m-%d'). Sizes only; no names or contents."

echo >&2
printf '%s' "$REPORT"

if command -v pbcopy >/dev/null 2>&1; then
  printf '%s' "$REPORT" | pbcopy
  echo >&2
  echo "Done. The report above has been copied to your clipboard; paste it back to the sender." >&2
else
  echo >&2
  echo "Done. Copy the report above and send it back." >&2
fi

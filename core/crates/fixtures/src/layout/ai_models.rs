//! Local AI models (PRD D12): Ollama, LM Studio (new and older folders) and
//! the Hugging Face cache. Each area also has files next to the models that
//! no rule may match: Ollama's private key, LM Studio's chats and the
//! Hugging Face sign-in token.

use super::KB;
use crate::builder::Builder;
use crate::paths;
use crate::FixtureError;

/// Builds the AI models part of the fixture.
pub fn build(b: &mut Builder) -> Result<(), FixtureError> {
    ollama(b)?;
    lm_studio(b)?;
    hugging_face(b)
}

/// One Ollama model: a manifest and two shared layers, plus Ollama's key
/// and history next to the models folder.
fn ollama(b: &mut Builder) -> Result<(), FixtureError> {
    let models = paths::OLLAMA_MODELS;
    b.text(
        &format!("{models}/manifests/registry.ollama.ai/library/llama3.2/latest"),
        "{\"schemaVersion\":2,\"layers\":[]}\n",
    )?;
    b.bytes(&format!("{models}/blobs/sha256-6a0746a1ec1a"), 32 * KB)?;
    b.bytes(&format!("{models}/blobs/sha256-4f659a1e86d7"), 8 * KB)?;
    b.text(
        paths::TRAP_OLLAMA_KEY,
        "fixture stand-in for Ollama's key, not a real key\n",
    )?;
    b.text(".ollama/history", "why is the sky blue\n")
}

/// Two models in LM Studio's folder and one in its older folder, plus a
/// saved chat that no rule may match.
fn lm_studio(b: &mut Builder) -> Result<(), FixtureError> {
    for m in paths::LM_STUDIO_MODEL_NAMES {
        b.bytes(
            &format!("{}/{m}/model.gguf", paths::LM_STUDIO_MODELS),
            24 * KB,
        )?;
    }
    b.bytes(
        &format!(
            "{}/{}/mistral-7b-instruct-v0.2.Q4_K_M.gguf",
            paths::LM_STUDIO_LEGACY_MODELS,
            paths::LM_STUDIO_LEGACY_MODEL
        ),
        24 * KB,
    )?;
    b.text(
        &format!(
            "{}/1730900000000.conversation.json",
            paths::TRAP_LM_STUDIO_CONVERSATIONS
        ),
        "{\"name\":\"Trip plan\"}\n",
    )
}

/// Two models and a dataset in the Hugging Face cache, plus the sign-in
/// token next to it.
fn hugging_face(b: &mut Builder) -> Result<(), FixtureError> {
    let hub = paths::HUGGINGFACE_HUB;
    for m in paths::HUGGINGFACE_MODELS {
        b.bytes(&format!("{hub}/{m}/blobs/3f2a9c"), 16 * KB)?;
        b.text(&format!("{hub}/{m}/refs/main"), "a1b2c3d4\n")?;
    }
    b.bytes(
        &format!("{hub}/{}/blobs/9e8d7c", paths::HUGGINGFACE_DATASET),
        16 * KB,
    )?;
    b.text(
        paths::TRAP_HUGGINGFACE_TOKEN,
        "hf_example_not_a_real_token\n",
    )
}

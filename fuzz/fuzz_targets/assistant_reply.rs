#![no_main]
//! A model server's answer: a chat reply and a model list in each provider's format, and
//! the usage it reports. The first byte picks the provider and the HTTP status.
use globecraft_assistant::{Api, HttpResponse, provider, usage};

libfuzzer_sys::fuzz_target!(|bytes: &[u8]| {
    let Some((pick, body)) = bytes.split_first() else {
        return;
    };
    let api = match pick % 3 {
        0 => Api::OpenAi,
        1 => Api::Anthropic,
        _ => Api::Gemini,
    };
    let status = [200, 400, 401, 429, 500][usize::from(pick / 3) % 5];
    let response = HttpResponse {
        status,
        body: body.to_vec(),
        headers: vec![("retry-after".to_owned(), "2".to_owned())],
    };
    let _ = provider::parse_reply(api, &response);
    let _ = provider::parse_models(api, &response);
    let _ = usage::read(body);
});

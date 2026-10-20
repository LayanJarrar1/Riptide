//! Real Windows Credential Manager round trip with a throwaway target, cleaned up by a guard.
//! Skips itself (passes) if the store is unavailable, e.g. a service session without a profile.
#![allow(missing_docs)]
#![cfg(windows)]

use riptide_ai::{ApiKey, CredentialManagerStore, ProviderKind, SecretStore};

struct Cleanup(CredentialManagerStore);
impl Drop for Cleanup {
    fn drop(&mut self) {
        for k in ProviderKind::ALL {
            let _ = self.0.delete(k);
        }
    }
}

#[test]
fn credential_manager_round_trip() {
    let prefix = format!("RiptideTest/ai/{}", std::process::id());
    let store = CredentialManagerStore::new(prefix);
    let _guard = Cleanup(store.clone());
    let key = ApiKey::new("sk-test-roundtrip-1234567890").expect("key");

    if let Err(e) = store.set(ProviderKind::Groq, &key) {
        eprintln!("SKIP: Credential Manager unavailable: {e}");
        return;
    }
    let back = store
        .get(ProviderKind::Groq)
        .expect("get")
        .expect("present");
    assert_eq!(back.expose(), key.expose());
    assert_eq!(back.masked(), "••••7890");
    assert!(store.get(ProviderKind::OpenAi).expect("get").is_none());

    // Overwrite.
    let key2 = ApiKey::new("sk-test-second-0987654321").expect("key");
    store.set(ProviderKind::Groq, &key2).expect("set");
    assert_eq!(
        store
            .get(ProviderKind::Groq)
            .expect("get")
            .expect("p")
            .expose(),
        key2.expose()
    );

    store.delete(ProviderKind::Groq).expect("delete");
    store
        .delete(ProviderKind::Groq)
        .expect("delete twice is fine");
    assert!(store.get(ProviderKind::Groq).expect("get").is_none());
}

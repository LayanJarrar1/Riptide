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

#[test]
fn named_secrets_round_trip_in_credential_manager() {
    let prefix = format!("RiptideTest/named/{}", std::process::id());
    let store = CredentialManagerStore::new(prefix);
    struct Gone(CredentialManagerStore);
    impl Drop for Gone {
        fn drop(&mut self) {
            let _ = self.0.delete_named("brave");
        }
    }
    let _g = Gone(store.clone());
    let key = ApiKey::new("brave-token-1234567890").expect("key");
    if let Err(e) = store.set_named("brave", &key) {
        eprintln!("SKIP: Credential Manager unavailable: {e}");
        return;
    }
    assert_eq!(
        store
            .get_named("brave")
            .expect("get")
            .expect("present")
            .expose(),
        key.expose()
    );
    assert!(store.get_named("jamendo").expect("get").is_none());
    store.delete_named("brave").expect("delete");
    assert!(store.get_named("brave").expect("get").is_none());
}

//! Manuální smoke test reálného OS keychainu. Nikdy neběží v CI (#[ignore] —
//! CI runner je Linux bez D-Bus/gnome-keyring a chování by se lišilo od
//! Windows/macOS, na které OsKeychain reálně cílí).
//!
//! Regrese pro bug: keyring 3.x na Windows zapisuje s CRED_PERSIST_ENTERPRISE,
//! což vyžaduje počítač připojený do domény (Active Directory). Na běžném
//! standalone Windows PC set_password tiše "projde", ale credential se
//! vůbec nezapíše — get_password pak vrátí NoEntry. Opraveno upgradem na
//! keyring 4.x (přepsaný Windows backend).
//!
//! Spouštět ručně: cargo test -p weave-infrastructure --test keychain_smoke -- --ignored

use weave_application::ports::keychain_port::{ApiService, KeychainPort};
use weave_infrastructure::keychain::OsKeychain;

#[tokio::test]
#[ignore = "zapisuje do reálného OS keychainu, spouštět ručně"]
async fn store_then_retrieve_roundtrips_on_this_machine() {
    let keychain = OsKeychain;
    let service = ApiService::CivitAi;
    let token = "smoke-test-token-12345";

    keychain.store(&service, token).await.expect("store selhal");

    let retrieved = keychain
        .retrieve(&service)
        .await
        .expect("retrieve selhal")
        .expect(
            "token by měl existovat hned po store — pokud None, keyring tiše nezapsal credential",
        );

    assert_eq!(retrieved, token);

    keychain.delete(&service).await.expect("delete selhal");
    let after_delete = keychain
        .retrieve(&service)
        .await
        .expect("retrieve po delete selhal");
    assert!(after_delete.is_none(), "token by po delete neměl existovat");
}

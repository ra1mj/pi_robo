mod support;

use pi_agent::ToolErrorCategory;
use pi_test_support::FakeCancellation;
use pi_tools::MutationCoordinator;
use std::sync::Arc;
use std::time::Duration;
use support::TempRoot;

#[tokio::test]
async fn symlink_aliases_share_one_mutation_lease() {
    let root = TempRoot::new("mutation-alias");
    let target = root.path().join("target.txt");
    let alias = root.path().join("alias.txt");
    std::fs::write(&target, "x").expect("seed file");
    std::os::unix::fs::symlink(&target, &alias).expect("create symlink");
    let coordinator = MutationCoordinator::default();
    let cancellation = Arc::new(FakeCancellation::default());
    let lease = coordinator
        .acquire(&target, cancellation.as_ref())
        .await
        .expect("first lease");

    let second_coordinator = coordinator.clone();
    let second_cancellation = Arc::clone(&cancellation);
    let second = tokio::spawn(async move {
        second_coordinator
            .acquire(&alias, second_cancellation.as_ref())
            .await
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(!second.is_finished());
    drop(lease);
    assert!(second.await.expect("lease task").is_ok());
}

#[tokio::test]
async fn waiting_for_a_lease_is_cancellable() {
    let root = TempRoot::new("mutation-cancel");
    let path = root.path().join("target.txt");
    std::fs::write(&path, "x").expect("seed file");
    let coordinator = MutationCoordinator::default();
    let first_cancel = FakeCancellation::default();
    let _lease = coordinator
        .acquire(&path, &first_cancel)
        .await
        .expect("first lease");
    let cancellation = Arc::new(FakeCancellation::default());
    let second_coordinator = coordinator.clone();
    let second_path = path.clone();
    let second_cancel = Arc::clone(&cancellation);
    let waiting = tokio::spawn(async move {
        second_coordinator
            .acquire(&second_path, second_cancel.as_ref())
            .await
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    cancellation.cancel();
    let error = waiting
        .await
        .expect("waiting task")
        .expect_err("cancelled lease");
    assert_eq!(error.category, ToolErrorCategory::Cancelled);
}

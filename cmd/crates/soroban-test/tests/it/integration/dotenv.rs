use soroban_test::TestEnv;
use super::util::deploy_hello;

fn write_env_file(e: &TestEnv, contents: &str) {
    let env_file = e.dir().join(".env");
    let contents = format!("SOROBAN_CONTRACT_ID={contents}");
    std::fs::write(&env_file, &contents).expect("Failed to write to .env file");
    let read_contents = std::fs::read_to_string(&env_file).expect("Failed to read .env file");
    assert_eq!(contents, read_contents, "Contents of .env do not match");
}

#[tokio::test]
async fn current_env_not_overwritten() {
    let e = TestEnv::new();
    write_env_file(&e, &deploy_hello(&e).await);
    e.new_assert_cmd("contract")
        .env(
            "SOROBAN_CONTRACT_ID",
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAFCT4",
        )
        .arg("invoke")
        .arg("--")
        .arg("hello")
        .arg("--world=world")
        .assert()
        .stderr(
            "❌ error: Contract not found: CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAFCT4\n",
        );
}


#[tokio::test]
async fn cli_args_have_priority() {
    let e = &TestEnv::new();
    let id = deploy_hello(e).await;
    write_env_file(e, &id);
    
    let result = e.new_assert_cmd("contract")
        .env("SOROBAN_CONTRACT_ID", "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAFCT4")
        .arg("invoke")
        .arg("--id")
        .arg(id)
        .arg("--")
        .arg("hello")
        .arg("--world=world")
        .assert();
    
    result.stdout("[\"Hello\",\"world\"]\n")
        .success();
}

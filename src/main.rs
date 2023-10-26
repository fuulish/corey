use reqwest::Error;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct User {
    login: String,
    id: u32,
}

// #[tokio::main] - using the blocking version should be fine for now
// this file should get updated on demand or rarely
fn main() -> Result<(), Error> {
    let request_url = format!(
        "https://api.github.com/repos/{owner}/{repo}/stargazers",
        owner = "rust-lang-nursery",
        repo = "rust-cookbook"
    );
    println!("{}", request_url);
    let client = reqwest::blocking::Client::builder()
        .user_agent("frankytanky")
        .build()?;
    let response = client.get(&request_url).send()?;

    let users: Vec<User> = response.json()?;
    println!("{:?}", users);
    Ok(())
}

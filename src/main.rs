use reqwest::Error;
use serde::Deserialize;

use std::env;

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
        owner = "fuulish",
        repo = "pong"
    );
    println!("{}", request_url);
    let token = env::var("TOKEN").unwrap();

    let client = reqwest::blocking::Client::new()
        .get(request_url)
        .header("User-Agent", "clireview/0.0.1")
        .bearer_auth(token);
    let response = client.send()?;
    /*
    let response = Client::new()
        .get(build_github_access_data_url())
        .header("Accept", "application/json")
        .header("User-Agent", "Rust")
        .bearer_auth(token)
        .send()
        .await?;
    */
    // .user_agent("clireview/0.0.1")

    let users: Vec<User> = response.json()?;
    println!("{:?}", users);
    Ok(())
}

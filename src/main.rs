use reqwest;
use serde::Deserialize;

use std::env;

#[derive(Debug)]
enum Error {
    Processing(env::VarError),
    Gathering(reqwest::Error),
}

#[derive(Deserialize, Debug)]
struct User {
    login: String,
}

#[derive(Deserialize, Debug)]
struct ReviewComment {
    id: u32, // too small?
    in_reply_to_id: Option<u32>,
    body: String,
    commit_id: String,
    original_commit_id: String,
    /// can be null through overwritten commit (force-push/rebase)
    line: Option<u32>,
    original_line: u32,
    user: User,
}

// rustlings does it like this:
impl Error {
    fn from_var_error(err: env::VarError) -> Error {
        Error::Processing(err)
    }

    fn from_reqwest_error(err: reqwest::Error) -> Error {
        Error::Gathering(err)
    }
}

// #[tokio::main] - using the blocking version should be fine for now
// this file should get updated on demand or rarely
fn main() -> Result<(), Error> {
    let request_url = format!(
        "https://api.github.com/repos/{owner}/{repo}/pulls/{prnum}/comments",
        owner = "fuulish",
        repo = "pong",
        prnum = 2,
    );
    println!("{}", request_url);
    let token = env::var("TOKEN").map_err(Error::from_var_error)?;

    let client = reqwest::blocking::Client::new()
        .get(request_url)
        .header("User-Agent", "clireview/0.0.1")
        .bearer_auth(token);
    let response = client.send().map_err(Error::from_reqwest_error)?;
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

    let users: Vec<ReviewComment> = response.json().map_err(Error::from_reqwest_error)?;
    println!("{:?}", users);
    Ok(())
}

use reqwest;
use serde::Deserialize;

use core::fmt;
use std::{collections::HashMap, env};

#[derive(Debug)]
enum Error {
    Processing(env::VarError),
    Gathering(reqwest::Error),
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.write_str(match self {
            Error::Processing(_) => "processing error",
            Error::Gathering(_) => "gathering error",
        })
    }
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
    diff_hunk: String,
    path: String,
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

enum Platform {
    Github(String),
}

impl Platform {
    fn get_token(&self) -> &str {
        match self {
            Platform::Github(ref token) => token,
        }
    }
}

struct PullRequest {
    platform: Platform,
    owner: String,
    repo: String,
    id: u32,
}

impl PullRequest {
    pub fn get_review_comments(&self) -> Result<Vec<ReviewComment>, Error> {
        let request_url = match self.platform {
            Platform::Github(_) => {
                format!(
                    "https://api.github.com/repos/{owner}/{repo}/pulls/{prnum}/comments",
                    owner = self.owner,
                    repo = self.repo,
                    prnum = self.id,
                )
            }
        };
        let client = reqwest::blocking::Client::new()
            .get(request_url)
            .header("User-Agent", "clireview/0.0.1")
            .bearer_auth(self.platform.get_token());

        let response = client.send().map_err(Error::from_reqwest_error)?;

        response.json().map_err(Error::from_reqwest_error)
    }
}

const NCOL: usize = 80;

// #[tokio::main] - using the blocking version should be fine for now
// this file should get updated on demand or rarely
fn main() -> Result<(), Error> {
    let token = env::var("TOKEN").map_err(Error::from_var_error)?;

    let pr = PullRequest {
        platform: Platform::Github(token),
        owner: "fuulish".to_owned(),
        repo: "pong".to_owned(),
        id: 2,
    };

    let comments = pr.get_review_comments()?;

    // XXX: sort into two hashmaps
    //      - one with original ids
    //      - one with replies to original ids

    let original_ids: HashMap<_, _> = comments
        .iter()
        .filter(|x| None == x.in_reply_to_id)
        .map(|c| (c.id, c))
        .collect();

    let mut reply_ids: HashMap<u32, Vec<&ReviewComment>> = HashMap::new();

    // XXX: depending on size of review, this will not be cheap
    //      however, for the typical size of reviews, we are talking
    //      about, this will not be expensive either
    //          XXX: optimize, when the need arises
    for (k, _) in original_ids.iter() {
        reply_ids.insert(
            *k,
            comments
                .iter()
                .filter(|x| {
                    if let Some(id) = x.in_reply_to_id {
                        if id == *k {
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                })
                .collect(),
        );
    }
    let reply_ids = reply_ids; // don't need this to be mutable any longer

    // pretty printing of conversations
    for (id, comment) in original_ids.iter() {
        // XXX: I always forget, do we need this explicit
        //      iter call?
        println!("|{}|", "+".repeat(NCOL));
        println!("{}", comment.path);
        println!("{}", comment.diff_hunk);
        println!(
            "{name}: {body}",
            name = comment.user.login,
            body = comment.body
        );
        match reply_ids.get(id) {
            None => {
                println!("|{}|", "-".repeat(NCOL));
                continue;
            }
            Some(rid) => {
                // XXX: sort replies by ids (if required)
                for reply in rid {
                    // XXX error handling
                    println!("{name}: {body}", name = reply.user.login, body = reply.body);
                }
                println!("|{}|", "-".repeat(NCOL));
            }
        }
    }
    Ok(())
}

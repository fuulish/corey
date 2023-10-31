use reqwest;
use serde::{Deserialize, Serialize};

use core::fmt;
use std::{collections::HashMap, env};

#[derive(Debug)]
enum Error {
    NotImplemented,
    Processing(env::VarError),
    Gathering(reqwest::Error),
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.write_str(match self {
            Error::Processing(_) => "processing error",
            Error::Gathering(_) => "gathering error",
            Error::NotImplemented => "not implemented",
        })
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct User {
    login: String,
}

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(ValueEnum, Debug, Copy, Clone, Serialize, Deserialize)]
enum ReviewInterface {
    GitHub,
}

#[derive(Debug, Serialize, Deserialize)]
struct Review {
    interface: ReviewInterface,
    token: String,
    owner: String,
    repo: String,
    url: String,
    id: u32,
    pub comments: Vec<ReviewComment>,
    // XXX: maybe add review comments here as well
    //      they do belong to the review, after all
}

impl Review {
    pub fn from_args(args: &Args) -> Result<Self, Error> {
        Ok(Review {
            interface: args.platform,
            token: args.token.to_owned(),
            owner: args.owner.to_owned(),
            repo: args.repo.to_owned(),
            url: args.url.to_owned(),
            id: args.id,
            comments: Review::get_review_comments(
                &args.owner,
                &args.repo,
                &args.url,
                args.id,
                &args.token,
                args.platform,
            )?,
        })
    }
    fn get_review_comments(
        owner: &str,
        repo: &str,
        url: &str,
        prnum: u32,
        token: &str,
        interface: ReviewInterface,
    ) -> Result<Vec<ReviewComment>, Error> {
        let request_url = match interface {
            ReviewInterface::GitHub => {
                format!(
                    "https://api.{url}/repos/{owner}/{repo}/pulls/{prnum}/comments",
                    owner = owner,
                    repo = repo,
                    url = url,
                    prnum = prnum,
                )
            }
        };
        let client = reqwest::blocking::Client::new()
            .get(request_url)
            .header("User-Agent", "clireview/0.0.1")
            .bearer_auth(&token);

        let response = client.send().map_err(Error::from_reqwest_error)?;

        response.json().map_err(Error::from_reqwest_error)
    }
}

const NCOL: usize = 80;

use clap::{Parser, ValueEnum};

// ValueEnum from here: https://strawlab.org/strand-braid-api-docs/latest/clap/trait.ValueEnum.html#example
#[derive(ValueEnum, Debug, Clone)]
enum Command {
    Init,
    Update,
}

// XXX: provide optional remote, otherwise see if .git directory is present and use default remote
//      is there a default remote? there is an upstream branch, could use that..., or simply
//      specify the remote to use - otherwise specify url directly
//
// XXX: can an enum with embedded value be used in input parsing?
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short = 't', long)]
    token: String,
    // XXX: all of those work, but which one is right
    // #[arg(value_enum)]
    // #[arg(short='t', long, value_enum)]
    // #[clap(value_enum)]
    #[clap(value_enum)]
    command: Command,
    #[arg(short = 'u', long)]
    url: String,
    #[arg(value_enum, short = 'p', long)]
    platform: ReviewInterface,
    #[arg(short = 'o', long)]
    owner: String,
    #[arg(short = 'r', long)]
    repo: String,
    #[arg(short = 'i', long)]
    id: u32,
}

// #[tokio::main] - using the blocking version should be fine for now
// this file should get updated on demand or rarely
fn main() -> Result<(), Error> {
    let args = Args::parse();

    let pr = match args.command {
        Command::Init => Review::from_args(&args)?,
        Command::Update => return Err(Error::NotImplemented), // read config and update comments
    };

    let f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open("review.yml")
        .expect("Couldn't open file");
    serde_yaml::to_writer(f, &pr).unwrap();

    // XXX: sort into two hashmaps
    //      - one with original ids
    //      - one with replies to original ids

    let original_ids: HashMap<_, _> = pr
        .comments
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
            pr.comments
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

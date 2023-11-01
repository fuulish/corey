use reqwest;
use serde::{Deserialize, Serialize};

use core::fmt;
use std::{collections::HashMap, env, fs};

#[derive(Debug)]
enum Error {
    NotImplemented,
    Processing(env::VarError),
    Gathering(reqwest::Error),
    IOError(std::io::Error),
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.write_str(match self {
            Error::Processing(_) => "processing error",
            Error::Gathering(_) => "gathering error",
            Error::NotImplemented => "not implemented",
            Error::IOError(_) => "I/O error",
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
    fn from_io_error(err: std::io::Error) -> Error {
        Error::IOError(err)
    }
}

#[derive(ValueEnum, Debug, Copy, Clone, Serialize, Deserialize)]
enum ReviewInterface {
    GitHub,
}

// XXX: a review should neglect the infrastructure things (to a large degree)
//      keep a list of comments and the views into it being original comments and replies
//      -> one private member and two public view
//      on the contrary, we'll keep only the review infrastructure things here
//      ...and treat the comments and a view into it separately
// XXX: move infrastructure related things to ReviewInterface (and have that use the platform enum)
#[derive(Debug, Serialize, Deserialize)]
struct Review {
    interface: ReviewInterface,
    owner: String,
    repo: String,
    auth: String,
    url: String,
    id: u32,
}

// XXX: having reviewcomments in here would have been nice
//      however, it generates a self-referential struct, which does not work out of the box in rust
struct Conversation<'a> {
    pub starter: HashMap<u32, &'a ReviewComment>,
    pub replies: HashMap<u32, Vec<&'a ReviewComment>>,
}

impl<'a> Conversation<'a> {
    pub fn from_review_comments(comments: &'a Vec<ReviewComment>) -> Result<Self, Error> {
        let starter: HashMap<_, _> = comments
            .iter()
            .filter(|x| None == x.in_reply_to_id)
            .map(|c| (c.id, c))
            .collect();

        let mut replies: HashMap<u32, Vec<&ReviewComment>> = HashMap::new();

        // XXX: depending on size of review, this will not be cheap
        //      however, for the typical size of reviews, we are talking
        //      about, this will not be expensive either
        //          XXX: optimize, when the need arises
        for (k, _) in starter.iter() {
            replies.insert(
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
        let replies = replies; // don't need this to be mutable any longer

        Ok(Conversation { starter, replies })
    }
    pub fn print(&self) {
        // pretty printing of conversations
        for (id, comment) in self.starter.iter() {
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
            match self.replies.get(id) {
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
    }
}

impl Review {
    pub fn from_args(args: &Args) -> Result<Self, Error> {
        Ok(Review {
            interface: args.platform,
            owner: args.owner.to_owned(),
            repo: args.repo.to_owned(),
            url: args.url.to_owned(),
            id: args.id,
            auth: args.token.to_owned(),
        })
    }

    fn get_authentication(&self) -> Result<String, Error> {
        fs::read_to_string(&self.auth).map_err(Error::from_io_error)
    }
    pub fn get_comments(&self) -> Result<Vec<ReviewComment>, Error> {
        let request_url = match self.interface {
            ReviewInterface::GitHub => {
                format!(
                    "https://api.{url}/repos/{owner}/{repo}/pulls/{prnum}/comments",
                    owner = self.owner,
                    repo = self.repo,
                    url = self.url,
                    prnum = self.id,
                )
            }
        };

        let token = self.get_authentication()?;

        let client = reqwest::blocking::Client::new()
            .get(request_url)
            .header("User-Agent", "clireview/0.0.1")
            .bearer_auth(&token);

        let response = client.send().map_err(Error::from_reqwest_error)?;

        response.json().map_err(Error::from_reqwest_error)
    }
    pub fn save_config(&self) -> Result<(), Error> {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open("review.yml")
            .expect("Couldn't open file");
        serde_yaml::to_writer(f, &self).unwrap(); // XXX: return proper error
        Ok(())
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

    pr.save_config().unwrap(); // XXX: return proper error

    let comments = pr.get_comments()?;
    let conversation = Conversation::from_review_comments(&comments)?;

    conversation.print();
    Ok(())
}

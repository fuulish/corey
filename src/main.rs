use reqwest;
use serde::{Deserialize, Serialize};

use core::fmt;
use std::{collections::HashMap, fs};

#[derive(Debug)]
enum Error {
    NotImplemented,
    MissingConfig,
    Gathering(reqwest::Error),
    IOError(std::io::Error),
    YAML(serde_yaml::Error),
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.write_str(match self {
            Error::YAML(_) => "YAML processing error",
            Error::Gathering(_) => "gathering error",
            Error::NotImplemented => "not implemented",
            Error::IOError(_) => "I/O error",
            Error::MissingConfig => "configuration incomplete",
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
    fn from_reqwest_error(err: reqwest::Error) -> Error {
        Error::Gathering(err)
    }
    fn from_io_error(err: std::io::Error) -> Error {
        Error::IOError(err)
    }
    fn from_yaml_error(err: serde_yaml::Error) -> Error {
        Error::YAML(err)
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
    comments: String, // XXX: I really do not want this to be part of this struct
                      //      XXX: re-extract and save comments to separate file
}

// XXX: having reviewcomments in here would have been nice
//      however, it generates a self-referential struct, which does not work out of the box in rust
//      it would be possible to setup starter and replies as Option types and only fill them after
//      having set up the base struct...
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
        // XXX: ensure that config filename and comments filename are not the same?
        Ok(Review {
            interface: args.platform,
            owner: args.owner.to_owned(),
            repo: args.repo.to_owned(),
            url: args.url.to_owned(),
            id: args.id,
            auth: args.token.to_owned(),
            comments: args.fname.to_owned(),
        })
    }

    /* update comments or both comments and comments file
    fn update_comments(&mut self) -> Result<(), Error> {
        self.comments = Review::get_comments(
            self.interface,
            &self.owner,
            &self.repo,
            &self.url,
            &self.auth,
            self.id,
        )?;
        Ok(())
    }
    */

    fn get_authentication(auth: &str) -> Result<String, Error> {
        fs::read_to_string(auth).map_err(Error::from_io_error)
    }
    fn get_comments(&self) -> Result<Vec<ReviewComment>, Error> {
        let request_url = match self.interface {
            ReviewInterface::GitHub => {
                format!(
                    "https://api.{url}/repos/{owner}/{repo}/pulls/{prnum}/comments",
                    owner = &self.owner,
                    repo = &self.repo,
                    url = &self.url,
                    prnum = self.id,
                )
            }
        };

        let token = Review::get_authentication(&self.auth)?;

        let client = reqwest::blocking::Client::new()
            .get(request_url)
            .header("User-Agent", "clireview/0.0.1")
            .bearer_auth(token);

        let response = client.send().map_err(Error::from_reqwest_error)?;

        response.json().map_err(Error::from_reqwest_error)
    }

    // XXX: deduplicate file saving
    pub fn save_config(&self) -> Result<(), Error> {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open("review.yml")
            .expect("Couldn't open file");
        serde_yaml::to_writer(f, &self).map_err(Error::from_yaml_error)
    }

    pub fn save_comments(&self, comments: &Vec<ReviewComment>) -> Result<(), Error> {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.comments)
            .expect("Couldn't open file");
        serde_yaml::to_writer(f, comments).map_err(Error::from_yaml_error)
    }

    pub fn from_config(config: &str) -> Result<Self, Error> {
        let f = std::fs::File::open(config).map_err(Error::from_io_error)?; // XXX: move to input
                                                                            // parm (opening is not
                                                                            // the responsibility
                                                                            // of this function)
        serde_yaml::from_reader(f).map_err(Error::from_yaml_error)
    }
}

const NCOL: usize = 80;

use clap::{Parser, ValueEnum};

// ValueEnum from here: https://strawlab.org/strand-braid-api-docs/latest/clap/trait.ValueEnum.html#example
#[derive(ValueEnum, Debug, Clone)]
enum Command {
    Init,
    Update,
    Run,
}

// XXX: provide optional remote, otherwise see if .git directory is present and use default remote
//      is there a default remote? there is an upstream branch, could use that..., or simply
//      specify the remote to use - otherwise specify url directly
//
// XXX: can an enum with embedded value be used in input parsing? (nope)
// XXX: turn initial setup args into optional arguments...
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
    #[arg(short = 'c', long)]
    config: Option<String>,
    #[arg(short = 'f', long)]
    fname: String,
}

// #[tokio::main] - using the blocking version should be fine for now
// this file should get updated on demand or rarely
fn main() -> Result<(), Error> {
    let args = Args::parse();

    let pr = match args.command {
        Command::Init => {
            let pr = Review::from_args(&args)?;
            pr.save_config()?;
            pr
        } // XXX: only create configuration
        Command::Update => match args.config {
            // XXX: only read configuration
            //      XXX: make sure that only this is set and other options are ignored
            Some(c) => Review::from_config(&c)?,
            None => return Err(Error::MissingConfig),
        },
        Command::Run => return Err(Error::NotImplemented),
    };

    // XXX: for init/update -> download comments into a Vec<ReviewComment>

    // XXX: save into args.config (how about making that optional?)
    //      i.e., how to represent optional arguments in serde

    let comments = pr.get_comments()?;
    pr.save_comments(&comments)?;

    let conversation = Conversation::from_review_comments(&comments)?;

    conversation.print();
    Ok(())
}

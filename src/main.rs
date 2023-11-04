//! Review LSP
//!
//! Provides LSP interfaces for reviewing code inline in editor.
use reqwest;
use serde::{Deserialize, Serialize};

use core::fmt;
use std::{collections::HashMap, fs};

const CONFIG_NAME: &'static str = ".review.yml";

#[allow(dead_code)]
#[derive(Debug)]
enum Error {
    NotImplemented,
    MissingConfig,
    InconsistentConfig,
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
            Error::InconsistentConfig => "configuration inconsistent",
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

// XXX: PartialEq needed for comparison in `from_args`
//      XXX: find nicer way to check for invalid values in `from_args` and remove it here?!:?!?!??!
#[derive(ValueEnum, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
enum ReviewInterface {
    GitHub,
}

/// A `Review` contains only the meta information
#[derive(Debug, Serialize, Deserialize)]
struct Review {
    interface: ReviewInterface,
    owner: String,
    repo: String,
    auth: String,
    url: String,
    id: u32,
    comments: String,
}

// cannot simply have original comments and references to it in one struct (self-referential)
// hence we provide a Conversation as a view into a list of ReviewComments
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
        for (id, comment) in &self.starter {
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
        let Some(interface) = args.platform else {
            return Err(Error::MissingConfig);
        };
        let Some(ref owner) = &args.owner else {
            return Err(Error::MissingConfig);
        };
        let Some(ref repo) = &args.repo else {
            return Err(Error::MissingConfig);
        };
        let Some(ref url) = &args.url else {
            return Err(Error::MissingConfig);
        };
        let Some(id) = args.id else {
            return Err(Error::MissingConfig);
        };
        let Some(ref auth) = &args.token else {
            return Err(Error::MissingConfig);
        };
        // XXX: input parsing might be easier with sensible default handling directly through clap
        //      https://stackoverflow.com/questions/55133351/is-there-a-way-to-get-clap-to-use-default-values-from-a-file
        let comments = match &args.fname {
            Some(v) => v.to_owned(),
            None => ".review_comments.yml".to_owned(),
        };

        Ok(Review {
            interface,
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            url: url.to_owned(),
            id,
            auth: auth.to_owned(),
            comments: comments.to_owned(),
        })
    }

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
            .open(CONFIG_NAME) // XXX: not configurable by default
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

    pub fn update_config(&mut self, args: &Args) -> Result<(), Error> {
        self.interface = match &args.platform {
            Some(v) => v.to_owned(),
            None => self.interface,
        };

        self.owner = match &args.owner {
            Some(v) => v.to_owned(),
            None => self.owner.to_owned(), // XXX: annoying copies should be avoided
        };

        self.repo = match &args.repo {
            Some(v) => v.to_owned(),
            None => self.repo.to_owned(),
        };
        self.url = match &args.url {
            Some(v) => v.to_owned(),
            None => self.url.to_owned(),
        };
        self.id = match args.id {
            Some(v) => v,
            None => self.id,
        };

        self.auth = match &args.token {
            Some(v) => v.to_owned(),
            None => self.auth.to_owned(),
        };

        self.comments = match &args.fname {
            Some(v) => v.to_owned(),
            None => self.comments.to_owned(),
        };

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
    token: Option<String>,
    // XXX: all of those work, but which one is right
    // #[arg(value_enum)]
    // #[arg(short='t', long, value_enum)]
    // #[clap(value_enum)]
    #[clap(value_enum)]
    command: Option<Command>,
    #[arg(short = 'u', long)]
    url: Option<String>,
    #[arg(value_enum, short = 'p', long)]
    platform: Option<ReviewInterface>,
    #[arg(short = 'o', long)]
    owner: Option<String>,
    #[arg(short = 'r', long)]
    repo: Option<String>,
    #[arg(short = 'i', long)]
    id: Option<u32>,
    #[arg(short = 'f', long)]
    fname: Option<String>,
}

fn serve_comments(review: &Review) -> Result<(), Error> {
    let comments = review.get_comments()?;
    review.save_comments(&comments)?;

    let conversation = Conversation::from_review_comments(&comments)?;

    conversation.print();

    Ok(())
}

// XXX: decide on semantics
//      init/update can refer solely to the configuration (there will be no updating of comments at
//      that stage)
//          e.g., only update the PR number that you are referring to
//          have a file watcher on running instance that notices if review things change
//              review things can be either the configuration or the pointed to comments file
//              then, re-serve the (possibly) updated comments
//      init/update can refer to the whole review
//
//      running without a command could also mean: read configuration file and serve comments on
//      LSP
//
//      - make the configuration file some sort of default
//          - that makes reading it from the serving side easier (when the whole things is served
//          from editor)

// #[tokio::main] - using the blocking version should be fine for now
// this file should get updated on demand or rarely
fn main() -> Result<(), Error> {
    let args = Args::parse();

    let command = match &args.command {
        Some(c) => c.clone(), // Command type could be `Copy`, though
        None => Command::Run,
    };

    let mut pr = match command {
        Command::Init => Review::from_args(&args)?,
        Command::Update | Command::Run => Review::from_config(CONFIG_NAME)?,
    };

    match command {
        Command::Update => pr.update_config(&args)?,
        _ => (),
    }
    let pr = pr;

    match command {
        Command::Init | Command::Update => pr.save_config()?,
        Command::Run => serve_comments(&pr)?,
    }
    Ok(())
}

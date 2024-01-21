//! Review LSP
//!
//! Provides LSP interfaces for reviewing code inline in editor.
use diff::Diff;
use reqwest;
use reqwest::Response;
use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::ServerCapabilities;

use bytes::Bytes;

use core::fmt;
use std::{collections::HashMap, fs};

use tower_lsp::jsonrpc;
use tower_lsp::lsp_types;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use git2;

mod diff;

#[allow(dead_code)]
#[derive(Debug)]
enum Error {
    SNH(String),
    NotImplemented,
    MissingConfig(String),
    InconsistentConfig,
    Gathering(reqwest::Error),
    IOError(std::io::Error),
    YAML(serde_yaml::Error),
    Git(git2::Error),
    UTF8Error(std::str::Utf8Error),
    RequestError(reqwest::StatusCode),
    DiffError,
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        // XXX there must be a better way than creating owned strings for all of those
        let msg = match self {
            Error::SNH(t) => format!("Should not happen {}", t),
            Error::Git(_) => "Git error".to_owned(),
            Error::YAML(_) => "YAML processing error".to_owned(),
            Error::Gathering(_) => "gathering error".to_owned(),
            Error::NotImplemented => "not implemented".to_owned(),
            Error::IOError(_) => "I/O error".to_owned(),
            Error::MissingConfig(miss) => format!("configuration incomplete: {} missing", miss),
            Error::InconsistentConfig => "configuration inconsistent".to_owned(),
            Error::UTF8Error(_) => "UTF8 decoding error".to_owned(),
            Error::RequestError(err) => format!("Request error: {}", err),
            Error::DiffError => format!("Error processing diff"),
        };
        f.write_str(&msg)
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
    start_line: Option<u32>,
    original_start_line: Option<u32>,
    user: User,
    diff_hunk: String,
    path: String, // XXX: should be OsString or something like that
    subject_type: Option<String>,
    start_side: Option<String>,
}

enum SubjectType {
    Line,
    File,
}

enum CommentSide {
    OriginalSide,
    Side,
}

// XXX: - ensure line-in-review to line-in-editor correspondence
//      - double-check meaning of lines in GH API
//      - only original_line appears to be mandatory
//      - in the only example used so far, it appears that we are hitting the correct lines only by
//      accident
//      - use VCS (preferably git) tracking to find the correct line in the current file
// XXX: GitHub uses 1-based lines and lsp_types::Range uses zero-based one
// XXX: fix understanding, but original is referring to a file from which was moved to another file
impl ReviewComment {
    // XXX: implement
    fn commented_side(&self) -> CommentSide {
        CommentSide::OriginalSide
    }

    fn get_subject_type(&self) -> SubjectType {
        match &self.subject_type {
            Some(subject) => {
                if let Some(v) = subject.find("line") {
                    SubjectType::Line
                } else if let Some(v) = subject.find("file") {
                    SubjectType::File
                } else {
                    SubjectType::Line // XXX: incorrect
                }
            }
            None => SubjectType::Line, // XXX: incorrect, need to check if original_line and things
                                       // are present
        }
    }
    // XXX: this is still very much GitHub specific
    #[cfg(feature = "debug")]
    async fn line_range(&self, text: &str, client: &Client) -> lsp_types::Range {
        // XXX: new algorithm:
        //      - check if line corresponds to the one in the diff
        //          YES: we are done
        //      - next check if we can find the proper context
        //          - reduce context until proper context found
        //              - calculate approximate new line location from diff notes

        let end = self.original_line; // range is exclusive, so 1-based inclusive end is fine for
                                      // zero-based exclusive end
        let beg = match self.original_start_line {
            Some(l) => l - 1, // start needs to be corrected, though
            None => end - 1,
        };

        let (beg, end) = match self.get_subject_type() {
            SubjectType::File => (beg, end),
            SubjectType::Line => {
                let line_diff = end - beg;

                let diff = Diff::from_only_hunk(&self.diff_hunk, &self.path).unwrap();

                // can go looking for text() and for original_text(), but it's more likely to be some
                // variation of test()
                let commented_on_text = diff.text(); // XXX: again, need to find correctly sided
                                                     // text
                                                     // XXX: add method to get enum to correctly
                                                     // access the commented on side

                client
                    .log_message(
                        lsp_types::MessageType::ERROR,
                        format!("FUX| commented on text: {}", commented_on_text),
                    )
                    .await;
                let beg: u32 = if commented_on_text.len() == 0 {
                    client
                        .log_message(lsp_types::MessageType::ERROR, "zero-length text")
                        .await;
                    beg
                } else {
                    match text.find(&commented_on_text) {
                        Some(index) => {
                            client
                                .log_message(lsp_types::MessageType::ERROR, "found text")
                                .await;
                            text[..index].matches("\n").count().try_into().unwrap()
                        }
                        None => {
                            client
                                .log_message(
                                    lsp_types::MessageType::ERROR,
                                    format!(
                                        "FUX| text: {} nowhere to be found in {}",
                                        commented_on_text, text
                                    ),
                                )
                                .await;
                            beg
                        }
                    }
                };

                let end = beg + line_diff;
                (beg, end)
            }
        };

        lsp_types::Range::new(
            lsp_types::Position::new(beg, 0),
            lsp_types::Position::new(end, 0),
        )

        /*
        // XXX: this needs to become a robust method returning a range for the various permutations
        // of line type types
        let diff_relative_line_no = self.original_line - diff.original_line_range().start;

        let commented_on_lines: Vec<_> = commented_on_text.split("\n").collect();
        let text_lines: Vec<_> = text.split("\n").collect();

        match text_lines[end as usize]
            .find(commented_on_lines[(diff_relative_line_no - 1) as usize])
        {
            Some(_) => lsp_types::Range::new(
                lsp_types::Position::new(beg, 0),
                lsp_types::Position::new(end, 0),
            ),
            None => lsp_types::Range::new(
                lsp_types::Position::new(beg, 0),
                lsp_types::Position::new(end, 0),
            ), // XXX: have this path continue with regular code execution
        }
            */

        // XXX: this is not how I thought this would go
    }
    #[cfg(not(feature = "debug"))]
    fn line_range(&self, text: &str) -> lsp_types::Range {
        // XXX: new algorithm:
        //      - check if line corresponds to the one in the diff
        //          YES: we are done
        //      - next check if we can find the proper context
        //          - reduce context until proper context found
        //              - calculate approximate new line location from diff notes

        let end = self.original_line; // range is exclusive, so 1-based inclusive end is fine for
                                      // zero-based exclusive end
        let beg = match self.original_start_line {
            Some(l) => l - 1, // start needs to be corrected, though
            None => end - 1,
        };

        let (beg, end) = match self.get_subject_type() {
            SubjectType::File => (beg, end),
            SubjectType::Line => {
                let line_diff = end - beg;

                let diff = Diff::from_only_hunk(&self.diff_hunk, &self.path).unwrap();

                // can go looking for text() and for original_text(), but it's more likely to be some
                // variation of test()
                let commented_on_text = diff.text(); // XXX: again, need to find correctly sided
                                                     // text
                                                     // XXX: add method to get enum to correctly
                                                     // access the commented on side

                let beg: u32 = if commented_on_text.len() == 0 {
                    beg
                } else {
                    match text.find(&commented_on_text) {
                        Some(index) => text[..index].matches("\n").count().try_into().unwrap(),
                        None => beg,
                    }
                };

                let end = beg + line_diff;
                (beg, end)
            }
        };

        lsp_types::Range::new(
            lsp_types::Position::new(beg, 0),
            lsp_types::Position::new(end, 0),
        )
    }
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
    fn from_git_error(err: git2::Error) -> Error {
        Error::Git(err)
    }
    fn from_utf8_error(err: std::str::Utf8Error) -> Error {
        Error::UTF8Error(err)
    }
    fn from_diff_error(err: diff::Error) -> Error {
        Error::DiffError
    }
}

// XXX: PartialEq needed for comparison in `from_args`
//      XXX: find nicer way to check for invalid values in `from_args` and remove it here?!:?!?!??!
#[derive(ValueEnum, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
enum ReviewInterface {
    GitHub,
}

enum VCS {
    Git(git2::Repository),
}

// XXX if it's only one member could use tuple struct
struct Repo {
    vcs: VCS,
}

impl Repo {
    fn new(interface: &ReviewInterface, local_repo: &str) -> Result<Repo, Error> {
        Ok(Repo {
            vcs: match interface {
                ReviewInterface::GitHub => {
                    VCS::Git(git2::Repository::open(local_repo).map_err(Error::from_git_error)?)
                }
            },
        })
    }
}

/* this seems unnecessary
impl Drop for Repo {
    fn drop(&mut self) {
        match self.vcs {
            VCS::Git(r) => r.drop(),
        }
    }
}
*/

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
    local_repo: String,
}

// cannot simply have original comments and references to it in one struct (self-referential)
// hence we provide a Conversation as a view into a list of ReviewComments
struct Conversation<'a> {
    pub starter: Vec<&'a ReviewComment>,
    pub replies: HashMap<u32, Vec<&'a ReviewComment>>,
}

impl<'a> Conversation<'a> {
    pub fn from_review_comments(comments: &'a Vec<ReviewComment>) -> Result<Self, Error> {
        let starter: Vec<_> = comments
            .iter()
            .filter(|x| None == x.in_reply_to_id)
            .collect();

        let mut replies: HashMap<u32, Vec<&ReviewComment>> = HashMap::new();

        // XXX: depending on size of review, this will not be cheap
        //      however, for the typical size of reviews, we are talking
        //      about, this will not be expensive either
        //          XXX: optimize, when the need arises
        for k in starter.iter() {
            replies.insert(
                k.id,
                comments
                    .iter()
                    .filter(|x| {
                        if let Some(id) = x.in_reply_to_id {
                            if id == k.id {
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
        for comment in &self.starter {
            println!("|{}|", "+".repeat(NCOL));
            println!("{}", comment.path);
            println!("{}", comment.diff_hunk);
            println!(
                "[{id}]{name}: {body}",
                id = comment.id,
                name = comment.user.login,
                body = comment.body
            );
            match self.replies.get(&comment.id) {
                None => {
                    println!("|{}|", "-".repeat(NCOL));
                    continue;
                }
                Some(rid) => {
                    // XXX: sort replies by ids (if required)
                    for reply in rid {
                        // XXX error handling
                        println!(
                            "[{id}]{name}: {body}",
                            id = reply.id,
                            name = reply.user.login,
                            body = reply.body
                        );
                    }
                    println!("|{}|", "-".repeat(NCOL));
                }
            }
        }
    }

    pub fn serialize(&self, start: &ReviewComment) -> String {
        let mut conv = format!("{}: {}", start.user.login, start.body);

        match self.replies.get(&start.id) {
            None => (),
            Some(rid) => {
                for reply in rid {
                    conv.push_str(&format!("\n{}: {}", reply.user.login, reply.body));
                }
            }
        }

        conv
    }
}

fn save_to_disk<T: Serialize>(fname: &str, data: &T) -> Result<(), Error> {
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(fname)
        .expect("Couldn't open file");
    serde_yaml::to_writer(f, data).map_err(Error::from_yaml_error)
}

impl Review {
    const CONFIG_NAME: &'static str = ".review.yml";

    pub fn from_args(args: &Args) -> Result<Self, Error> {
        let Some(interface) = args.platform else {
            return Err(Error::MissingConfig("interface".to_owned()));
        };
        let Some(ref owner) = &args.owner else {
            return Err(Error::MissingConfig("owner".to_owned()));
        };
        let Some(ref repo) = &args.repo else {
            return Err(Error::MissingConfig("reposiotry".to_owned()));
        };
        let Some(ref url) = &args.url else {
            return Err(Error::MissingConfig("URL".to_owned()));
        };
        let Some(id) = args.id else {
            return Err(Error::MissingConfig("PR ID".to_owned()));
        };
        let Some(ref auth) = &args.token else {
            return Err(Error::MissingConfig("authentication".to_owned()));
        };

        let local_repo = match &args.local_repo {
            Some(r) => r.to_owned(),
            None => "./".to_owned(),
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
            local_repo,
        })
    }

    fn get_authentication(auth: &str) -> Result<String, Error> {
        fs::read_to_string(auth).map_err(Error::from_io_error)
    }
    async fn get_comments_response(&self) -> Result<Response, Error> {
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

        let res = reqwest::Client::new()
            .get(request_url)
            .header("User-Agent", "clireview/0.0.1")
            .bearer_auth(token)
            .send()
            .await
            .map_err(Error::from_reqwest_error)?;

        return match res.error_for_status_ref() {
            Ok(_) => Ok(res),
            Err(err) => match err.status() {
                Some(v) => Err(Error::RequestError(v)),
                None => Err(Error::SNH("something went wrong in weeds".to_owned())),
            },
        };
    }

    async fn get_comments(&self) -> Result<Vec<ReviewComment>, Error> {
        self.get_comments_response()
            .await?
            .json()
            .await
            .map_err(Error::from_reqwest_error)
    }

    async fn raw_comments(&self) -> Result<Bytes, Error> {
        self.get_comments_response()
            .await?
            .bytes()
            .await
            .map_err(Error::from_reqwest_error)
    }

    pub fn save_config(&self) -> Result<(), Error> {
        save_to_disk(Self::CONFIG_NAME, self)
    }

    pub fn save_comments(&self, comments: &Vec<ReviewComment>) -> Result<(), Error> {
        save_to_disk(&self.comments, comments)
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

        self.local_repo = match &args.local_repo {
            Some(v) => v.to_owned(),
            None => self.local_repo.to_owned(),
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
    Print,
    Raw,
    Reply,
    Comment,
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
    #[arg(short = 'l', long)]
    local_repo: Option<String>,
    #[arg(short = 'c', long)]
    comment: Option<u32>,
    #[arg(short = 'b', long)]
    body: Option<String>,
    #[arg(short = 'j', long)]
    commit_id: Option<String>,
    #[arg(short = 'x', long)]
    path: Option<String>,
}

// XXX: use `register_capability` to register new capabilities
// XXX: text synchronization capabilities as in tower-lsp-boilerplate
// XXX: publish_diagnostics on `open` and on `did_change` -> that might need document
//      synchronization capabilities
// XXX: need to publish new diagnostics upon review change (i.e., another file watcher)
//      -> how is that done? just sent asynchronously?
//      -> did_change_configuration might be of use here as well
// XXX: include client in backend
//      or rather, create a backend struct that includes a review

struct Backend {
    client: Client,
    review: Review,
}

impl Backend {
    async fn on_change(&self, params: lsp_types::TextDocumentItem) {
        let comments = match self.review.get_comments().await {
            Ok(v) => v,
            Err(e) => {
                self.client
                    .log_message(lsp_types::MessageType::ERROR, e.to_string())
                    .await;
                return;
            }
        };

        let conversation = match Conversation::from_review_comments(&comments) {
            Ok(v) => v,
            Err(e) => {
                self.client
                    .log_message(lsp_types::MessageType::ERROR, e.to_string())
                    .await;
                return;
            }
        };

        let repo = match Repo::new(&self.review.interface, &self.review.local_repo) {
            Ok(r) => r,
            Err(e) => {
                self.client
                    .log_message(lsp_types::MessageType::ERROR, e.to_string())
                    .await;
                return;
            }
        };

        // XXX: only for debugging purposes
        //      BUT: note that the full document text is coming through
        //      we can use that within a rope and search for the text that is within the actual
        //      commit
        #[cfg(feature = "message_tracing")]
        self.client
            .log_message(
                lsp_types::MessageType::ERROR,
                format!("FUX| text is: {}", params.text),
            )
            .await;

        let uri = params.uri.as_str();

        // XXX: also need to figure out what exactly is being sent by GitHub
        //      should always be the line and the commit ID, so we can blame it directly and also
        //      compare to what we're having at this moment

        // line range
        //  params.text contains the string of interest
        //  -> can turn it into a rope and use that for more info
        //
        // check commit id
        // check cleanliness of commit,
        // if everything is clean, `line_range` is just fine
        // if it's unclean or on another commit, we need git magic
        // unclean:
        //  compare lines from text document and the params.text
        //  check how file evolved and whether the line of interest is still present or what it has
        //  morphed into
        #[cfg(feature = "debug")]
        let diagnostics: Vec<lsp_types::Diagnostic> = futures::future::join_all(
            conversation
                .starter
                .iter()
                .filter(|x| uri.contains(&x.path))
                // XXX: the line_range below is only correct if we are on the same version as on review
                //      XXX: need to fix this line association using git internals
                //      for now, this is good enough
                .map(|x| async {
                    lsp_types::Diagnostic::new_simple(
                        x.line_range(&params.text, &self.client).await,
                        conversation.serialize(x),
                    )
                }),
        )
        .await;
        #[cfg(not(feature = "debug"))]
        let diagnostics = conversation
            .starter
            .iter()
            .filter(|x| uri.contains(&x.path))
            // XXX: the line_range below is only correct if we are on the same version as on review
            //      XXX: need to fix this line association using git internals
            //      for now, this is good enough
            .map(|&x| {
                lsp_types::Diagnostic::new_simple(
                    x.line_range(&params.text),
                    conversation.serialize(&x),
                )
            })
            .collect();

        self.client
            .publish_diagnostics(params.uri.clone(), diagnostics, Some(params.version))
            .await;
    }
}

#[tower_lsp::async_trait] // XXX is this needed? Y: otherwise Rust will complain about
                          // lifetime bounds of trait
impl LanguageServer for Backend {
    async fn initialize(
        &self,
        _: lsp_types::InitializeParams,
    ) -> jsonrpc::Result<lsp_types::InitializeResult> {
        Ok(lsp_types::InitializeResult {
            server_info: None,
            // offset_encoding: None, // XXX: was in tower-lsp-boilerplate, why not here?
            capabilities: ServerCapabilities {
                // The only thing we want to provide are `textDocument/diagnostic` respsonses.
                // This does not need to register its own client and server capabilities.
                // ...however, the server can register for the textDocument/diagnostic capability
                text_document_sync: Some(lsp_types::TextDocumentSyncCapability::Kind(
                    lsp_types::TextDocumentSyncKind::FULL,
                )),
                ..ServerCapabilities::default()
            },
        })
    }
    async fn shutdown(&self) -> jsonrpc::Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: lsp_types::DidOpenTextDocumentParams) {
        self.client
            .log_message(lsp_types::MessageType::INFO, "file opened!")
            .await;
        self.on_change(lsp_types::TextDocumentItem {
            uri: params.text_document.uri,
            language_id: "X".to_owned(),
            text: params.text_document.text,
            version: params.text_document.version,
        })
        .await
    }

    async fn did_change(&self, mut params: lsp_types::DidChangeTextDocumentParams) {
        #[cfg(feature = "message_tracing")]
        self.client
            .log_message(
                lsp_types::MessageType::ERROR,
                format!("FUX| received textDocument/didChange notification"),
            )
            .await;
        self.on_change(lsp_types::TextDocumentItem {
            uri: params.text_document.uri,
            language_id: "X".to_owned(),
            text: std::mem::take(&mut params.content_changes[0].text),
            version: params.text_document.version,
        })
        .await
    }

    async fn did_save(&self, _: lsp_types::DidSaveTextDocumentParams) {
        self.client
            .log_message(lsp_types::MessageType::INFO, "file saved!")
            .await;
    }
    async fn did_close(&self, _: lsp_types::DidCloseTextDocumentParams) {
        self.client
            .log_message(lsp_types::MessageType::INFO, "file closed!")
            .await;
    }
}

async fn serve_comments(review: Review) -> Result<(), Error> {
    let comments = review.get_comments().await?; // XXX: always update from fresh source? (or use
                                                 //      available data/comments)
    review.save_comments(&comments)?;

    let repo = Repo::new(&review.interface, &review.local_repo)?;

    let (service, socket) = LspService::new(|client| Backend { client, review });

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}

async fn print_comments(review: Review) -> Result<(), Error> {
    let comments = review.get_comments().await?;
    review.save_comments(&comments)?;

    let conversation = Conversation::from_review_comments(&comments)?;
    conversation.print();

    Ok(())
}

async fn print_raw(review: Review) -> Result<(), Error> {
    let comments = review.raw_comments().await?;
    print!(
        "{}",
        std::str::from_utf8(&comments).map_err(Error::from_utf8_error)?
    );

    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct Reply {
    body: String,
}

async fn reply_to_comment(
    review: Review,
    id: Option<u32>,
    body: Option<String>,
) -> Result<(), Error> {
    let body = match body {
        Some(b) => b,
        None => return Err(Error::MissingConfig("reply body".to_owned())),
    };
    let id = match id {
        Some(i) => i,
        None => return Err(Error::MissingConfig("ID".to_owned())),
    };

    let token = Review::get_authentication(&review.auth)?;

    let request_body = Reply { body };

    let client = reqwest::Client::new();
    let res = client
        .post(
            format!("https://api.{URL}/repos/{OWNER}/{REPO}/pulls/{PULL_NUMBER}/comments/{COMMENT_ID}/replies",
                URL = &review.url,
                OWNER = &review.owner,
                REPO = &review.repo,
                PULL_NUMBER = review.id,
                COMMENT_ID = id),
        )
        .json(&request_body)
        .header("User-Agent", "clireview/0.0.1")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(token)
        .send()
        .await
        .map_err(Error::from_reqwest_error)?;

    return match res.error_for_status_ref() {
        Ok(_) => Ok(()),
        Err(err) => match err.status() {
            Some(v) => Err(Error::RequestError(v)),
            None => Err(Error::SNH("something went wrong in weeds".to_owned())),
        },
    };
}

#[derive(Debug, Serialize, Deserialize)]
struct Comment {
    body: String,
    commit_id: String,
    path: String,
}

async fn create_comment(
    review: Review,
    commit_id: Option<String>,
    body: Option<String>,
    path: Option<String>,
) -> Result<(), Error> {
    let body = match body {
        Some(b) => b,
        None => return Err(Error::MissingConfig("comment body".to_owned())),
    };
    let commit_id = match commit_id {
        Some(o) => o,
        None => return Err(Error::MissingConfig("commit ID".to_owned())),
    };
    let path = match path {
        Some(p) => p,
        None => return Err(Error::MissingConfig("relative file path".to_owned())),
    };

    let request_body = Comment {
        body,
        commit_id,
        path,
    };
    let token = Review::get_authentication(&review.auth)?;
    let client = reqwest::Client::new();

    let res = client
        .post(format!(
            "https://api.{URL}/repos/{OWNER}/{REPO}/pulls/{PULL_NUMBER}/comments",
            URL = &review.url,
            OWNER = &review.owner,
            REPO = &review.repo,
            PULL_NUMBER = review.id,
        ))
        .json(&request_body)
        .header("User-Agent", "clireview/0.0.1")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(token)
        .send()
        .await
        .map_err(Error::from_reqwest_error)?;

    return match res.error_for_status_ref() {
        Ok(_) => Ok(()),
        Err(err) => match err.status() {
            Some(v) => Err(Error::RequestError(v)),
            None => Err(Error::SNH("something went wrong in weeds".to_owned())),
        },
    };
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
//
// XXX: need some more tracking with respect to state of current files
//      -> use VCS in place to verify file version correspondence
//      -> are the files/lines we are looking at the same that the review is referring to?

// this file should get updated on demand or rarely
#[tokio::main]
async fn main() -> Result<(), Error> {
    let args = Args::parse();

    let command = match &args.command {
        Some(c) => c.clone(), // Command type could be `Copy`, though
        None => Command::Run,
    };

    let mut pr = match command {
        Command::Init => Review::from_args(&args)?,
        Command::Update
        | Command::Run
        | Command::Print
        | Command::Raw
        | Command::Reply
        | Command::Comment => Review::from_config(Review::CONFIG_NAME)?,
    };

    pr.update_config(&args)?;
    let pr = pr;

    match command {
        Command::Init | Command::Update => pr.save_config()?,
        Command::Run => serve_comments(pr).await?,
        Command::Print => print_comments(pr).await?,
        Command::Raw => print_raw(pr).await?,
        Command::Comment => create_comment(pr, args.commit_id, args.body, args.path).await?,
        Command::Reply => reply_to_comment(pr, args.comment, args.body).await?,
    }
    Ok(())
}

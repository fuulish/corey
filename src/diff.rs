use core::fmt;

#[allow(dead_code)]
#[derive(Debug)]
pub enum Error {
    Parse,
    Invalid,
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.write_str(match self {
            Error::Parse => "Could not parse hunk",
            Error::Invalid => "Hunk invalid",
        })
    }
}

use std::num::ParseIntError;

impl Error {
    fn from_parse_int_error(err: ParseIntError) -> Error {
        Error::Invalid
    }
}

// XXX: this work, but it's not pretty
pub struct Diff {
    path: String, // XXX: use std::path::Path?
    // XXX: also include original_path? (not needed, ATM)
    original_range: std::ops::Range<u32>,
    range: std::ops::Range<u32>,
    original_lines: std::vec::Vec<String>,
    lines: std::vec::Vec<String>,
    // original_context: std::vec::Vec<std::ops::Range<u32>>,
    context: std::vec::Vec<std::ops::Range<u32>>, // only one context needed, because it's common
    // XXX: unless there are movements between files
    // XXX: we are dealing with single file diffs,
    // here, though
    //    XXX: are multi-file diffs possible in
    //    github in terms of comments to diffs
    //    XXX: should be able to handle those as well
    //    XXX: multi-file diffs do not share context,
    //    do they?
    original_context: std::vec::Vec<std::ops::Range<u32>>,
    trailing_newline: bool,
}

#[derive(PartialEq)]
enum LineType {
    Context,
    Addition,
    Deletion,
}

impl Diff {
    // for this, we currently assume minimal hunks with no context overlap
    // in particular for the context generation
    //  XXX: fix this
    pub fn from_only_hunk(hunk: &str, path: &str) -> Result<Diff, Error> {
        if !hunk.starts_with("@@") {
            return Err(Error::Parse);
        }

        let trailing_newline = if hunk.ends_with("\n") { true } else { false };
        let mut original_lines = std::vec::Vec::<String>::new();
        let mut lines = std::vec::Vec::<String>::new();

        let mut start: u32 = 0;
        let mut stop: u32 = 0;
        let mut original_start: u32 = 0;
        let mut original_stop: u32 = 0;

        let mut context = std::vec::Vec::<std::ops::Range<u32>>::new();
        let mut original_context = std::vec::Vec::<std::ops::Range<u32>>::new();
        let mut context_start: u32 = 0; // 0 is not a valid line number (how about using something uninitialized?)
        let mut original_context_start: u32 = 0; // 0 is not a valid line number (how about using something uninitialized?)

        let mut previous_line_type = LineType::Context;

        // XXX: move first iteration out and simplify loop
        for line in hunk.split('\n') {
            if line.starts_with("@@") {
                // XXX: pull this out of the loop and create a proper
                // iterator over the rest of the data
                let mut data = line
                    .trim_start_matches("@")
                    .trim_start_matches(" ")
                    .trim_end_matches(" ")
                    .trim_end_matches("@");

                let mut start_index = data.find("-").ok_or(Error::Parse)? + 1;
                let stop_index = data.find(" ").ok_or(Error::Parse)?;

                let mut comma_sep = data[start_index..stop_index].split(",");
                start = comma_sep
                    .next()
                    .ok_or(Error::Parse)?
                    .parse::<u32>()
                    .map_err(Error::from_parse_int_error)?;
                stop = start; // stop will be calculated from how many lines there are in the patch
                              // XXX: could add cross-checking/precalculation here for later
                              // verification
                context_start = start;

                start_index = data.find("+").ok_or(Error::Parse)? + 1;
                data = data[start_index..].trim_start_matches(" "); // XXX start trim not necessary

                comma_sep = data.split(",");
                original_start = comma_sep
                    .next()
                    .ok_or(Error::Parse)?
                    .parse::<u32>()
                    .map_err(Error::from_parse_int_error)?;
                original_stop = start;
                original_context_start = original_start;
            } else {
                let line_type = if line.starts_with(" ") {
                    LineType::Context
                } else if line.starts_with("-") {
                    LineType::Deletion
                } else if line.starts_with("+") {
                    LineType::Addition
                } else {
                    return Err(Error::Invalid);
                };

                // XXX: neither addition of first or last line is always correct
                //      could remove the last newline by comparing to received diff...
                match line_type {
                    LineType::Context => {
                        original_lines.push(line[1..].to_owned());
                        original_stop += 1;

                        lines.push(line[1..].to_owned());
                        stop += 1;
                    }
                    LineType::Addition => {
                        lines.push(line[1..].to_owned());
                        stop += 1;
                    }
                    LineType::Deletion => {
                        original_lines.push(line[1..].to_owned());
                        original_stop += 1;
                    }
                }

                if previous_line_type != line_type {
                    match line_type {
                        LineType::Context => {
                            // start new context
                            context_start = stop - 1;
                            original_context_start = original_stop - 1;
                        }
                        LineType::Addition | LineType::Deletion => {
                            if previous_line_type == LineType::Context {
                                context.push(context_start..stop);
                                original_context.push(original_context_start..original_stop);
                            }
                        }
                    }
                }
                previous_line_type = line_type;
            }
        }

        Ok(Diff {
            path: path.to_owned(),
            original_range: original_start..original_stop,
            range: start..stop,
            original_lines,
            lines,
            context,
            original_context,
            trailing_newline,
        })
    }

    pub fn text(&self) -> String {
        // XXX: how about instead implementing std::fmt::Display or
        // something similar
        let mut out = String::new();

        for line in &self.lines {
            out.push_str(&line);
            out.push_str("\n"); // XXX: superfluous?/could check hunk if it contains a trailing \n
        }

        if !self.trailing_newline {
            out = out.strip_suffix("\n").unwrap().to_owned();
        }

        return out;
    }

    pub fn original_text(&self) -> String {
        let mut out = String::new();

        for line in &self.original_lines {
            out.push_str(&line);
            out.push_str("\n"); // XXX: superfluous?
        }

        if !self.trailing_newline {
            out = out.strip_suffix("\n").unwrap().to_owned();
        }

        return out;
    }

    /*
    pub fn context(&self, ignore_starting: Option<u32>) -> String {
        for i in self.
    }

    pub fn original_context(&self, ignore_starting: Option<u32>) -> String {}
    */
}

// the typical expectation is that the context is not changed, but rather the already changed lines
// simple assumption -> context stays the same (not necessarily true)
//
// hence, find preceding and following context and mark location as approximate (in particular if
// the line numbers don't fit)
//
// in general, it could be possible that a comment refers to a range that includes multiple changes
// github lets you select multiple diffs, but only acknowledges a range including the last change
// in the selection
//
// if there's no context, then we need to find another way :)
//
// we can also do fuzzy searching

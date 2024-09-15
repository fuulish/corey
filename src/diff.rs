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

use std::{num::ParseIntError, ops::Range};

use crate::CommentSide;

impl Error {
    fn from_parse_int_error(err: ParseIntError) -> Error {
        Error::Invalid
    }
}

pub struct LinePair(u32, u32);

// XXX: this work, but it's not pretty
pub struct Diff {
    path: String, // XXX: use std::path::Path?
    #[cfg(feature = "theFuture")]
    original_path: String, // XXX: use proper path, also
    // XXX: also include original_path? (not needed, ATM)
    #[cfg(feature = "debug")]
    pub original_range: std::ops::Range<u32>,
    #[cfg(not(feature = "debug"))]
    original_range: std::ops::Range<u32>,
    range: std::ops::Range<u32>,
    left_lines: std::vec::Vec<String>,
    right_lines: std::vec::Vec<String>,
    context: std::vec::Vec<std::ops::Range<u32>>, // only one context needed, because it's common
    // XXX: unless there are movements between files
    // XXX: we are dealing with single file diffs,
    // here, though
    //    XXX: are multi-file diffs possible in
    //    github in terms of comments to diffs
    //    XXX: should be able to handle those as well
    //    XXX: multi-file diffs do not share context,
    //    do they?
    associated_line_pairs: std::vec::Vec<LinePair>,
    trailing_newline: bool,
}

#[derive(PartialEq)]
enum LineType {
    Context,
    Addition,
    Deletion,
}

impl Diff {
    // XXX: pretty useless
    pub fn original_line_range(&self) -> std::ops::Range<u32> {
        self.original_range.start..self.original_range.end
    }
    // for this, we currently assume minimal hunks with no context overlap
    // in particular for the context generation
    //  XXX: fix this
    pub fn from_only_hunk(hunk: &str, path: &str) -> Result<Diff, Error> {
        if !hunk.starts_with("@@") {
            return Err(Error::Parse);
        }

        let trailing_newline = if hunk.ends_with("\n") { true } else { false };
        let mut left_lines = std::vec::Vec::<String>::new();
        let mut right_lines = std::vec::Vec::<String>::new();

        let mut right_start: u32 = 0;
        let mut right_stop: u32 = 0;
        let mut left_start: u32 = 0;
        let mut left_stop: u32 = 0;

        let mut associated_line_pairs: Vec<LinePair> = Vec::new();

        let mut context = std::vec::Vec::<std::ops::Range<u32>>::new();
        let mut context_start: u32 = 0; // 0 is not a valid line number (how about using something uninitialized?)

        let mut previous_line_type = LineType::Context;

        // XXX: also extract path and original_path from diff hunk

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
                let original_stop_index = data.find(" ").ok_or(Error::Parse)?;

                let mut comma_sep = data[start_index..original_stop_index].split(",");
                left_start = comma_sep
                    .next()
                    .ok_or(Error::Parse)?
                    .parse::<u32>()
                    .map_err(Error::from_parse_int_error)?;
                left_stop = left_start; // stop will be calculated from how many lines there are in the patch
                                        // XXX: could add cross-checking/precalculation here for later
                                        // verification

                start_index = data.find("+").ok_or(Error::Parse)? + 1;
                data = data[start_index..].trim_start_matches(" "); // XXX start trim not necessary

                comma_sep = data.split(",");
                right_start = comma_sep
                    .next()
                    .ok_or(Error::Parse)?
                    .parse::<u32>()
                    .map_err(Error::from_parse_int_error)?;
                right_stop = right_start;
                context_start = right_start;

                associated_line_pairs.push(LinePair(left_start, right_start));
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
                        left_lines.push(line[1..].to_owned());
                        left_stop += 1;

                        right_lines.push(line[1..].to_owned());
                        right_stop += 1;
                    }
                    LineType::Addition => {
                        right_lines.push(line[1..].to_owned());
                        right_stop += 1;
                    }
                    LineType::Deletion => {
                        left_lines.push(line[1..].to_owned());
                        left_stop += 1;
                    }
                }

                // XXX: there needs to be an associated initial push, somewhere
                associated_line_pairs.push(LinePair(left_stop, right_stop));

                if previous_line_type != line_type {
                    match line_type {
                        LineType::Context => {
                            // start new context
                            context_start = right_stop - 1;
                        }
                        LineType::Addition | LineType::Deletion => {
                            if previous_line_type == LineType::Context {
                                context.push(context_start..right_stop);
                            }
                        }
                    }
                }
                previous_line_type = line_type;
            }
        }

        Ok(Diff {
            path: path.to_owned(),
            original_range: left_start..left_stop,
            range: right_start..right_stop,
            left_lines,
            right_lines,
            context,
            associated_line_pairs,
            trailing_newline,
        })
    }

    pub fn text(&self) -> String {
        // XXX: how about instead implementing std::fmt::Display or
        // something similar
        let mut out = String::new();

        for line in &self.right_lines {
            out.push_str(&line);
            out.push_str("\n"); // XXX: superfluous?/could check hunk if it contains a trailing \n
        }

        if !self.trailing_newline {
            out = match out.strip_suffix("\n") {
                Some(v) => v.to_owned(),
                None => out,
            };
        }

        return out;

        // XXX: alternatively, simply always trim the last newline
        // out.trim_end_matches("\n").to_owned()
        // XXX: always trimming would only be a problem if someone came and marked that single
        // line-ending character
    }

    pub async fn text_part(&self, comment: Range<u32>, side: CommentSide) -> Result<String, Error> {
        let mut out = String::new();

        // XXX: this needs to be from the correct side (original might not be the one...)
        // XXX: use arg to function to choose respective range

        // let (lines, diff_line_range) = match side {
        // XXX: should this be a function?
        // XXX: debug start and end
        let (lines, text_start, text_end) = match side {
            CommentSide::LR | CommentSide::RL => panic!("not implemented"),
            CommentSide::LL => (
                &self.left_lines,
                self.associated_line_pairs[0].0,
                self.associated_line_pairs[self.associated_line_pairs.len() - 1].0,
            ),
            CommentSide::RR => (
                &self.right_lines,
                self.associated_line_pairs[0].1,
                self.associated_line_pairs[self.associated_line_pairs.len() - 1].1,
                // XXX: is there really no other way to get the last element of a vector
            ),
        };

        // let (start, end) = match side {
        //     CommentSide::LL | CommentSide::RL | CommentSide::LR => {
        //         (self.original_range.start, self.original_range.end)
        //     }
        //     CommentSide::RR => (self.range.start, self.range.end),
        // };
        if comment.start < text_start || comment.end > text_end {
            return Err(Error::Invalid);
        }
        // XXX: this looks wrong
        let start = comment.start - text_start;
        let end = start + comment.end - comment.start;

        for line_index in start..end {
            out.push_str(&lines[line_index as usize]);
            out.push_str("\n"); // XXX: superfluous?/could check hunk if it contains a trailing \n
        }

        if !self.trailing_newline {
            out = match out.strip_suffix("\n") {
                Some(v) => v.to_owned(),
                None => out,
            };
        }

        return Ok(out);
    }

    pub fn original_text(&self) -> String {
        let mut out = String::new();

        for line in &self.left_lines {
            out.push_str(&line);
            out.push_str("\n"); // XXX: superfluous?
        }

        if !self.trailing_newline {
            out = match out.strip_suffix("\n") {
                Some(v) => v.to_owned(),
                None => out,
            };
        }

        return out;

        // XXX: alternatively, simply always trim the last newline
        // out.trim_end_matches("\n").to_owned()
    }

    // XXX: this guy should return an iterator of String because we might have multiple contexts
    //      also supply properly expected line numbers to be able to judge if after or before
    //      change
    // XXX: implement missing _ignore_starting stuff
    pub fn get_context(&self, _ignore_starting: Option<u32>) -> Option<Vec<String>> {
        let mut res = Vec::<String>::new();

        if self.context.len() == 0 {
            return None;
        }

        for ctx in self.context.iter() {
            let mut tmp = String::new();
            for i in ctx.to_owned() {
                tmp.push_str(&self.right_lines[(i - self.range.start) as usize]);
                tmp.push_str("\n");
            }

            res.push(tmp.to_owned());
        }

        // XXX: remove trailing_whitespace if required

        Some(res)
    }
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

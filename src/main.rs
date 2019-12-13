use atty::{is, Stream};
use std::io::{self, BufRead};
use std::iter::Peekable;
use std::time::SystemTime;
use termcolor::{
    Color,
    Color::{Green, Red, White},
    ColorChoice, ColorSpec, StandardStream, WriteColor,
};

use diffr_lib::optimize_partition;
use diffr_lib::{DiffInput, HashedSpan, LineSplit, Snake, Tokenization};

mod cli_args;

#[derive(Debug)]
pub struct AppConfig {
    debug: bool,
    added_face: ColorSpec,
    refine_added_face: ColorSpec,
    removed_face: ColorSpec,
    refine_removed_face: ColorSpec,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            debug: false,
            added_face: color_spec(Some(Green), None, false),
            refine_added_face: color_spec(Some(White), Some(Green), true),
            removed_face: color_spec(Some(Red), None, false),
            refine_removed_face: color_spec(Some(White), Some(Red), true),
        }
    }
}

fn main() {
    let matches = cli_args::get_matches();
    if is(Stream::Stdin) {
        eprintln!("{}", matches.usage());
        std::process::exit(-1)
    }

    let mut config = AppConfig::default();
    config.debug = matches.is_present(cli_args::FLAG_DEBUG);

    if let Some(values) = matches.values_of(cli_args::FLAG_COLOR) {
        if let Err(err) = cli_args::parse_color_args(&mut config, values) {
            eprintln!("{}", err);
            std::process::exit(-1)
        }
    }

    match try_main(config) {
        Ok(()) => (),
        Err(ref err) if err.kind() == io::ErrorKind::BrokenPipe => (),
        Err(ref err) => {
            eprintln!("io error: {}", err);
            std::process::exit(-1)
        }
    }
}

fn now(do_timings: bool) -> Option<SystemTime> {
    if do_timings {
        Some(SystemTime::now())
    } else {
        None
    }
}

fn duration_ms_since(time: &Option<SystemTime>) -> u128 {
    if let Some(time) = time {
        if let Ok(elapsed) = time.elapsed() {
            elapsed.as_millis()
        } else {
            // some non monotonically increasing clock
            // this is a short period of time anyway,
            // let us map it to 0
            0
        }
    } else {
        0
    }
}

fn try_main(config: AppConfig) -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = StandardStream::stdout(ColorChoice::Always);
    let mut buffer = vec![];
    let mut hunk_buffer = HunkBuffer::new(config);
    let mut stdin = stdin.lock();
    let mut stdout = stdout.lock();
    let mut in_hunk = false;

    // process hunks
    loop {
        stdin.read_until(b'\n', &mut buffer)?;
        if buffer.is_empty() {
            break;
        }

        match (in_hunk, first_after_escape(&buffer)) {
            (true, Some(b'+')) => hunk_buffer.push_added(&buffer),
            (true, Some(b'-')) => hunk_buffer.push_removed(&buffer),
            (true, Some(b' ')) => add_raw_line(&mut hunk_buffer.lines, &buffer),
            (_, other) => {
                if in_hunk {
                    hunk_buffer.process_with_stats(&mut stdout)?;
                }
                in_hunk = other == Some(b'@');
                output(&buffer, 0, buffer.len(), &ColorSpec::default(), &mut stdout)?;
            }
        }
        buffer.clear();
    }

    // flush remaining hunk
    hunk_buffer.process_with_stats(&mut stdout)?;
    hunk_buffer.stats.stop();
    hunk_buffer.stats.report()?;
    Ok(())
}

fn color_spec(fg: Option<Color>, bg: Option<Color>, bold: bool) -> ColorSpec {
    let mut colorspec: ColorSpec = ColorSpec::default();
    colorspec.set_fg(fg);
    colorspec.set_bg(bg);
    colorspec.set_bold(bold);
    colorspec
}

#[derive(Default)]
struct ExecStats {
    time_computing_diff_ms: u128,
    time_lcs_ms: u128,
    time_opt_lcs_ms: u128,
    total_time_ms: u128,
    program_start: Option<SystemTime>,
}

impl ExecStats {
    fn new(debug: bool) -> Self {
        ExecStats {
            time_computing_diff_ms: 0,
            time_lcs_ms: 0,
            time_opt_lcs_ms: 0,
            total_time_ms: 0,
            program_start: now(debug),
        }
    }

    /// Should we call SystemTime::now at all?
    fn do_timings(&self) -> bool {
        self.program_start.is_some()
    }

    fn stop(&mut self) {
        if self.do_timings() {
            self.total_time_ms = duration_ms_since(&self.program_start);
        }
    }

    fn report(&self) -> std::io::Result<()> {
        self.report_into(&mut std::io::stderr())
    }

    fn report_into<W>(&self, w: &mut W) -> std::io::Result<()>
    where
        W: std::io::Write,
    {
        const WORD_PADDING: usize = 35;
        const FIELD_PADDING: usize = 15;
        if self.do_timings() {
            let format_header = |name| format!("{} (ms)", name);
            let format_ratio = |dt: u128| {
                format!(
                    "({:3.3}%)",
                    100.0 * (dt as f64) / (self.total_time_ms as f64)
                )
            };
            let mut report = |name: &'static str, dt: u128| {
                writeln!(
                    w,
                    "{:>w$} {:>f$} {:>f$}",
                    format_header(name),
                    dt,
                    format_ratio(dt),
                    w = WORD_PADDING,
                    f = FIELD_PADDING,
                )
            };
            report("hunk processing time", self.time_computing_diff_ms)?;
            report("-- compute lcs", self.time_lcs_ms)?;
            report("-- optimize lcs", self.time_opt_lcs_ms)?;
            writeln!(
                w,
                "{:>w$} {:>f$}",
                format_header("total processing time"),
                self.total_time_ms,
                w = WORD_PADDING,
                f = FIELD_PADDING,
            )?;
        }
        Ok(())
    }
}

struct HunkBuffer {
    v: Vec<isize>,
    diff_buffer: Vec<Snake>,
    added_tokens: Vec<HashedSpan>,
    removed_tokens: Vec<HashedSpan>,
    lines: LineSplit,
    config: AppConfig,
    stats: ExecStats,
}

fn shared_spans(added_tokens: &Tokenization, diff_buffer: &Vec<Snake>) -> Vec<HashedSpan> {
    let mut shared_spans = vec![];
    for snake in diff_buffer.iter() {
        for i in 0..snake.len {
            shared_spans.push(added_tokens.nth_span(snake.y0 + i));
        }
    }
    shared_spans
}

impl HunkBuffer {
    fn new(config: AppConfig) -> Self {
        let debug = config.debug;
        HunkBuffer {
            v: vec![],
            diff_buffer: vec![],
            added_tokens: vec![],
            removed_tokens: vec![],
            lines: Default::default(),
            config,
            stats: ExecStats::new(debug),
        }
    }

    // Returns the number of completely printed snakes
    fn paint_line<Stream, Positions>(
        data: &[u8],
        &(data_lo, data_hi): &(usize, usize),
        no_highlight: &ColorSpec,
        highlight: &ColorSpec,
        shared: &mut Peekable<Positions>,
        out: &mut Stream,
    ) -> io::Result<()>
    where
        Stream: WriteColor,
        Positions: Iterator<Item = (usize, usize)>,
    {
        let mut y = data_lo + 1;
        // XXX: skip leading token and leading spaces
        while y < data_hi && data[y].is_ascii_whitespace() {
            y += 1
        }
        output(data, data_lo, y, &no_highlight, out)?;
        while let Some((lo, hi)) = shared.peek() {
            if data_hi <= y {
                break;
            }
            let last_iter = data_hi <= *hi;
            let lo = (*lo).min(data_hi).max(y);
            let hi = (*hi).min(data_hi);
            if hi <= data_lo {
                shared.next();
                continue;
            }
            if hi < lo {
                continue;
            }
            output(data, y, lo, &highlight, out)?;
            output(data, lo, hi, &no_highlight, out)?;
            y = hi;
            if last_iter {
                break;
            } else {
                shared.next();
            }
        }
        output(data, y, data_hi, &highlight, out)?;
        Ok(())
    }

    fn process_with_stats<Stream>(&mut self, out: &mut Stream) -> io::Result<()>
    where
        Stream: WriteColor,
    {
        let start = now(self.stats.do_timings());
        let result = self.process(out);
        self.stats.time_computing_diff_ms += duration_ms_since(&start);
        result
    }

    fn process<Stream>(&mut self, out: &mut Stream) -> io::Result<()>
    where
        Stream: WriteColor,
    {
        let Self {
            v,
            diff_buffer,
            added_tokens,
            removed_tokens,
            lines,
            config,
            stats,
        } = self;
        let data = lines.data();
        let tokens = DiffInput {
            removed: Tokenization::new(lines.data(), removed_tokens),
            added: Tokenization::new(lines.data(), added_tokens),
        };
        let start = now(stats.do_timings());
        diffr_lib::diff(&tokens, v, diff_buffer);
        // TODO output the lcs directly out of `diff` instead
        let shared_spans = shared_spans(&tokens.added, &diff_buffer);
        let lcs = &Tokenization::new(tokens.added.data(), &shared_spans);
        stats.time_lcs_ms += duration_ms_since(&start);
        let start = now(stats.do_timings());
        let normalized_lcs_added = optimize_partition(&tokens.added, lcs);
        let normalized_lcs_removed = optimize_partition(&tokens.removed, lcs);
        stats.time_opt_lcs_ms += duration_ms_since(&start);
        let mut shared_added = normalized_lcs_added
            .shared_segments(&tokens.added)
            .peekable();
        let mut shared_removed = normalized_lcs_removed
            .shared_segments(&tokens.removed)
            .peekable();

        for (line_start, line_end) in lines.iter() {
            let first = data[line_start];
            match first {
                b'-' | b'+' => {
                    let is_plus = first == b'+';
                    let (nohighlight, highlight, toks, shared) = if is_plus {
                        (
                            &config.added_face,
                            &config.refine_added_face,
                            &tokens.added,
                            &mut shared_added,
                        )
                    } else {
                        (
                            &config.removed_face,
                            &config.refine_removed_face,
                            &tokens.removed,
                            &mut shared_removed,
                        )
                    };
                    Self::paint_line(
                        toks.data(),
                        &(line_start, line_end),
                        &nohighlight,
                        &highlight,
                        shared,
                        out,
                    )?;
                }
                _ => output(data, line_start, line_end, &ColorSpec::default(), out)?,
            }
        }
        lines.clear();
        added_tokens.clear();
        removed_tokens.clear();
        Ok(())
    }

    fn push_added(&mut self, line: &[u8]) {
        self.push_aux(line, true)
    }

    fn push_removed(&mut self, line: &[u8]) {
        self.push_aux(line, false)
    }

    fn push_aux(&mut self, line: &[u8], added: bool) {
        let mut ofs = self.lines.len() + 1;
        add_raw_line(&mut self.lines, line);
        // XXX: skip leading token and leading spaces
        while ofs < line.len() && line[ofs].is_ascii_whitespace() {
            ofs += 1
        }
        diffr_lib::tokenize(
            &self.lines.data(),
            ofs,
            if added {
                &mut self.added_tokens
            } else {
                &mut self.removed_tokens
            },
        );
    }
}

// TODO count whitespace characters as well here
fn add_raw_line(dst: &mut LineSplit, line: &[u8]) {
    let mut i = 0;
    let len = line.len();
    while i < len {
        i += skip_all_escape_code(&line[i..]);
        let tok_len = skip_token(&line[i..]);
        dst.append_line(&line[i..i + tok_len]);
        i += tok_len;
    }
}

fn output<Stream>(
    buf: &[u8],
    from: usize,
    to: usize,
    colorspec: &ColorSpec,
    out: &mut Stream,
) -> io::Result<()>
where
    Stream: WriteColor,
{
    let to = to.min(buf.len());
    if from >= to {
        return Ok(());
    }
    let buf = &buf[from..to];
    let ends_with_newline = buf.last().cloned() == Some(b'\n');
    let buf = if ends_with_newline {
        &buf[..buf.len() - 1]
    } else {
        buf
    };
    out.set_color(colorspec)?;
    out.write_all(&buf)?;
    out.reset()?;
    if ends_with_newline {
        out.write_all(b"\n")?;
    }
    Ok(())
}

/// Returns the number of bytes of escape code that start the slice.
fn skip_all_escape_code(buf: &[u8]) -> usize {
    // Skip one sequence
    fn skip_escape_code(buf: &[u8]) -> Option<usize> {
        if 2 <= buf.len() && &buf[..2] == b"\x1b[" {
            // "\x1b[" + sequence body + "m" => 3 additional bytes
            Some(index_of(&buf[2..], b'm')? + 3)
        } else {
            None
        }
    }
    let mut buf = buf;
    let mut sum = 0;
    while let Some(nbytes) = skip_escape_code(&buf) {
        buf = &buf[nbytes..];
        sum += nbytes
    }
    sum
}

/// Returns the first byte of the slice, after skipping the escape
/// code bytes.
fn first_after_escape(buf: &[u8]) -> Option<u8> {
    let nbytes = skip_all_escape_code(&buf);
    buf.iter().skip(nbytes).cloned().next()
}

/// Scan the slice looking for the given byte, returning the index of
/// its first appearance.
fn index_of(buf: &[u8], target: u8) -> Option<usize> {
    let mut it = buf.iter().enumerate();
    loop {
        match it.next() {
            Some((index, c)) => {
                if *c == target {
                    return Some(index);
                }
            }
            None => return None,
        }
    }
}

/// Computes the number of bytes until either the next escape code, or
/// the end of buf.
fn skip_token(buf: &[u8]) -> usize {
    match buf.len() {
        0 => 0,
        len => {
            for i in 0..buf.len() - 1 {
                if &buf[i..i + 2] == b"\x1b[" {
                    return i;
                }
            }
            len
        }
    }
}

#[cfg(test)]
mod test;

#[cfg(test)]
mod test_cli;

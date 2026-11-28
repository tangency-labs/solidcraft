//! Text folding, multi-token matching, highlights and typo-safe scoring for global search
//! (RFC 0019 §3.1).
//!
//! Matching is case- and accent-folded, deterministic, and bounded so hostile queries or long
//! names cannot stall search:
//! - Exact > prefix > word start > contiguous substring > ordered subsequence > typo fallback.
//! - Tokens shorter than 3 characters match contiguous substrings (including exact/prefix/word
//!   start) only.
//! - Typo tolerance (bounded Damerau-Levenshtein) **never** touches a token containing a digit
//!   (`M5` never matches `M6`; `MGN12` never matches `MGN15`), and only applies to tokens of at
//!   least 5 characters (`1` edit for 5–7 chars, up to `2` edits for 8+ chars).

use std::ops::Range;

use super::{MatchConfidence, MatchKind, MatchSpan, SearchField, SearchViewer};

/// Upper bound on candidate text length examined per field (characters).
pub const MAX_CANDIDATE_CHARS: usize = 512;
/// Upper bound on token/word length compared by bounded Damerau-Levenshtein.
pub const MAX_TYPO_CHARS: usize = 64;
/// Upper bound on query length examined by [`score_text`] / [`match_query_against_fields`].
pub const MAX_QUERY_CHARS: usize = 256;
/// Upper bound on whitespace-separated tokens per query.
pub const MAX_QUERY_TOKENS: usize = 16;

/// Case- and accent-folded representation of a candidate string, retaining the original UTF-8
/// byte range for every folded character so match highlights stay valid on the source text.
#[derive(Clone, Debug)]
pub struct FoldedText {
    chars: Vec<char>,
    byte_ranges: Vec<(usize, usize)>,
    words: Vec<WordSlice>,
}

#[derive(Clone, Copy, Debug)]
struct WordSlice {
    start: usize,
    end: usize,
}

impl FoldedText {
    pub fn new(raw: &str) -> Self {
        let mut chars = Vec::new();
        let mut byte_ranges = Vec::new();
        let mut raw_is_upper = Vec::new();

        for (byte_idx, ch) in raw.char_indices().take(MAX_CANDIDATE_CHARS) {
            let end_byte = byte_idx + ch.len_utf8();
            let upper = ch.is_uppercase();
            if let Some(folded) = fold_latin_accent(ch) {
                for fc in folded.chars() {
                    chars.push(fc);
                    byte_ranges.push((byte_idx, end_byte));
                    raw_is_upper.push(upper);
                }
            } else {
                for fc in ch.to_lowercase() {
                    chars.push(fc);
                    byte_ranges.push((byte_idx, end_byte));
                    raw_is_upper.push(upper);
                }
            }
        }

        let words = split_words(&chars, &raw_is_upper);
        Self { chars, byte_ranges, words }
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn char_len(&self) -> usize {
        self.chars.len()
    }

    /// Map a half-open folded character range back to a UTF-8 byte range in the original text.
    fn byte_span(&self, range: Range<usize>) -> Option<MatchSpan> {
        if range.start >= range.end {
            return None;
        }
        let start = self.byte_ranges.get(range.start)?.0;
        let end = self.byte_ranges.get(range.end - 1)?.1;
        (start < end).then_some(MatchSpan { start, end })
    }
}

/// Fold a string to lowercase, ASCII-accent-folded text (bounded to `max_chars`).
pub fn fold_str(raw: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(max_chars * 2));
    for ch in raw.chars().take(max_chars) {
        if let Some(folded) = fold_latin_accent(ch) {
            out.push_str(folded);
        } else {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
        }
    }
    out
}

/// Fold common Latin diacritics and ligatures into plain lowercase ASCII equivalents without
/// pulling in external Unicode normalization crates.
fn fold_latin_accent(ch: char) -> Option<&'static str> {
    Some(match ch {
        'À'..='Å' | 'à'..='å' | 'Ā' | 'ā' | 'Ă' | 'ă' | 'Ą' | 'ą' => "a",
        'Æ' | 'æ' => "ae",
        'Ç' | 'ç' | 'Ć' | 'ć' | 'Ĉ' | 'ĉ' | 'Ċ' | 'ċ' | 'Č' | 'č' => "c",
        'Ď' | 'ď' | 'Đ' | 'đ' => "d",
        'È'..='Ë' | 'è'..='ë' | 'Ē' | 'ē' | 'Ĕ' | 'ĕ' | 'Ė' | 'ė' | 'Ę' | 'ę' | 'Ě' | 'ě' => "e",
        'Ĝ' | 'ĝ' | 'Ğ' | 'ğ' | 'Ġ' | 'ġ' | 'Ģ' | 'ģ' => "g",
        'Ĥ' | 'ĥ' | 'Ħ' | 'ħ' => "h",
        'Ì'..='Ï' | 'ì'..='ï' | 'Ĩ' | 'ĩ' | 'Ī' | 'ī' | 'Ĭ' | 'ĭ' | 'Į' | 'į' | 'İ' | 'ı' => "i",
        'Ĵ' | 'ĵ' => "j",
        'Ķ' | 'ķ' => "k",
        'Ĺ' | 'ĺ' | 'Ļ' | 'ļ' | 'Ľ' | 'ľ' | 'Ŀ' | 'ŀ' | 'Ł' | 'ł' => "l",
        'Ñ' | 'ñ' | 'Ń' | 'ń' | 'Ņ' | 'ņ' | 'Ň' | 'ň' => "n",
        'Ò'..='Ö' | 'Ø' | 'ò'..='ö' | 'ø' | 'Ō' | 'ō' | 'Ŏ' | 'ŏ' | 'Ő' | 'ő' => "o",
        'Œ' | 'œ' => "oe",
        'Ŕ' | 'ŕ' | 'Ŗ' | 'ŗ' | 'Ř' | 'ř' => "r",
        'Ś' | 'ś' | 'Ŝ' | 'ŝ' | 'Ş' | 'ş' | 'Š' | 'š' => "s",
        'ß' | 'ẞ' => "ss",
        'Ţ' | 'ţ' | 'Ť' | 'ť' | 'Ŧ' | 'ŧ' => "t",
        'Ù'..='Ü' | 'ù'..='ü' | 'Ũ' | 'ũ' | 'Ū' | 'ū' | 'Ŭ' | 'ŭ' | 'Ů' | 'ů' | 'Ű' | 'ű' | 'Ų' | 'ų' => "u",
        'Ŵ' | 'ŵ' => "w",
        'Ý' | 'ý' | 'ÿ' | 'Ŷ' | 'ŷ' | 'Ÿ' => "y",
        'Ź' | 'ź' | 'Ż' | 'ż' | 'Ž' | 'ž' => "z",
        _ => return None,
    })
}

/// Split folded characters into word slices on non-alphanumeric boundaries and camelCase jumps.
fn split_words(chars: &[char], raw_is_upper: &[bool]) -> Vec<WordSlice> {
    let mut words = Vec::new();
    let mut start: Option<usize> = None;
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if let Some(s) = start.take() {
                words.push(WordSlice { start: s, end: i });
            }
            continue;
        }
        let prev_upper = i.checked_sub(1).and_then(|p| raw_is_upper.get(p)).copied().unwrap_or(false);
        let curr_upper = raw_is_upper.get(i).copied().unwrap_or(false);
        if let Some(s) = start {
            // Split camelCase transitions (`appBar` -> `app`, `Bar`).
            if curr_upper && !prev_upper && i > s {
                words.push(WordSlice { start: s, end: i });
                start = Some(i);
            }
        } else {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        words.push(WordSlice { start: s, end: chars.len() });
    }
    words
}

/// Whether a token contains any digit (RFC 0019 §3.1: typo tolerance never touches these).
pub fn token_has_digit(token: &[char]) -> bool {
    token.iter().any(|c| c.is_numeric())
}

/// Maximum allowed typo edit distance for a single token.
/// Returns `0` for digit-bearing tokens and tokens shorter than 5 characters.
pub fn max_typo_distance(token: &[char]) -> usize {
    if token_has_digit(token) || token.len() < 5 {
        0
    } else if token.len() >= 8 {
        2
    } else {
        1
    }
}

/// Bounded Optimal String Alignment (Damerau-Levenshtein) distance: single-character
/// substitution, insertion, deletion, or adjacent transposition.
///
/// Returns `Some(distance)` if `distance <= max_dist`, otherwise `None`.
pub fn damerau_levenshtein(a: &[char], b: &[char], max_dist: usize) -> Option<usize> {
    if a.len() > MAX_TYPO_CHARS || b.len() > MAX_TYPO_CHARS {
        return None;
    }
    if a.len().abs_diff(b.len()) > max_dist {
        return None;
    }
    if a == b {
        return Some(0);
    }
    if max_dist == 0 {
        return None;
    }
    let m = a.len();
    let n = b.len();
    let inf = max_dist + 1;
    let mut prev2 = vec![inf; n + 1];
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![inf; n + 1];

    for i in 1..=m {
        if let Some(slot) = curr.first_mut() {
            *slot = i;
        }
        let mut row_min = i;
        let ai = a.get(i - 1).copied();
        let ai_prev = i.checked_sub(2).and_then(|k| a.get(k)).copied();

        for j in 1..=n {
            let bj = b.get(j - 1).copied();
            let cost = usize::from(ai != bj);
            let del = prev.get(j).copied().unwrap_or(inf).saturating_add(1);
            let ins = curr.get(j - 1).copied().unwrap_or(inf).saturating_add(1);
            let sub = prev.get(j - 1).copied().unwrap_or(inf).saturating_add(cost);
            let mut best = del.min(ins).min(sub);

            if i > 1 && j > 1 {
                let bj_prev = b.get(j - 2).copied();
                if ai == bj_prev && ai_prev == bj {
                    let trans = prev2.get(j - 2).copied().unwrap_or(inf).saturating_add(1);
                    best = best.min(trans);
                }
            }
            if let Some(slot) = curr.get_mut(j) {
                *slot = best;
            }
            if best < row_min {
                row_min = best;
            }
        }
        if row_min > max_dist {
            return None;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut curr);
    }

    let dist = prev.get(n).copied().unwrap_or(inf);
    (dist <= max_dist).then_some(dist)
}

/// Unbounded (length-capped at 128 chars) Damerau-Levenshtein distance between two folded strings,
/// useful for "Did you mean?" suggestions when a query returns zero matches.
pub fn suggestion_distance(query: &str, candidate: &str) -> usize {
    let a: Vec<char> = fold_str(query.trim(), 128).chars().collect();
    let b: Vec<char> = fold_str(candidate.trim(), 128).chars().collect();
    let m = a.len();
    let n = b.len();
    let mut prev2 = vec![0usize; n + 1];
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];

    for i in 1..=m {
        if let Some(slot) = curr.first_mut() {
            *slot = i;
        }
        let ai = a.get(i - 1).copied();
        let ai_prev = i.checked_sub(2).and_then(|k| a.get(k)).copied();
        for j in 1..=n {
            let bj = b.get(j - 1).copied();
            let cost = usize::from(ai != bj);
            let del = prev.get(j).copied().unwrap_or(128).saturating_add(1);
            let ins = curr.get(j - 1).copied().unwrap_or(128).saturating_add(1);
            let sub = prev.get(j - 1).copied().unwrap_or(128).saturating_add(cost);
            let mut best = del.min(ins).min(sub);
            if i > 1 && j > 1 && ai == b.get(j - 2).copied() && ai_prev == bj {
                best = best.min(prev2.get(j - 2).copied().unwrap_or(128).saturating_add(1));
            }
            if let Some(slot) = curr.get_mut(j) {
                *slot = best;
            }
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut curr);
    }
    prev.get(n).copied().unwrap_or(128)
}

#[derive(Clone, Debug)]
struct TokenHit {
    kind: MatchKind,
    score: u32,
    span: Option<MatchSpan>,
}

/// Match a single folded token against one folded candidate string.
fn match_token(tok: &[char], cand: &FoldedText, allow_subsequence: bool, allow_typo: bool) -> Option<TokenHit> {
    if tok.is_empty() || cand.is_empty() {
        return None;
    }
    // 1. Exact match on full text.
    if cand.chars == tok {
        return Some(TokenHit { kind: MatchKind::Exact, score: 1200, span: cand.byte_span(0..cand.chars.len()) });
    }
    // 2. Prefix match on full text.
    if cand.chars.starts_with(tok) {
        let bonus = 1000u32.saturating_sub(cand.chars.len().min(300) as u32);
        return Some(TokenHit { kind: MatchKind::Prefix, score: bonus.max(700), span: cand.byte_span(0..tok.len()) });
    }
    // 3. Word-start match (exact word preferred over word prefix).
    let mut best_word: Option<(u32, Range<usize>)> = None;
    for (idx, word) in cand.words.iter().enumerate() {
        let Some(slice) = cand.chars.get(word.start..word.end) else { continue };
        if slice.starts_with(tok) {
            let exact_bonus: u32 = if slice.len() == tok.len() { 80 } else { 0 };
            let pos_penalty = (idx as u32).min(40) * 2;
            let s = (800u32 + exact_bonus).saturating_sub(pos_penalty);
            if best_word.as_ref().is_none_or(|(best_s, _)| s > *best_s) {
                best_word = Some((s, word.start..(word.start + tok.len())));
            }
        }
    }
    if let Some((s, range)) = best_word {
        return Some(TokenHit { kind: MatchKind::WordStart, score: s, span: cand.byte_span(range) });
    }
    // 4. Contiguous substring match.
    if let Some(pos) = find_subslice(&cand.chars, tok) {
        let s = 600u32.saturating_sub((pos as u32).min(150));
        return Some(TokenHit { kind: MatchKind::Substring, score: s.max(400), span: cand.byte_span(pos..(pos + tok.len())) });
    }

    let has_digit = token_has_digit(tok);
    // Tokens under 3 chars or containing digits match contiguous substrings only.
    if tok.len() < 3 || has_digit {
        return None;
    }

    // 5. Ordered subsequence match (letters in order with bounded gaps).
    if allow_subsequence && let Some((gaps, first, last)) = match_subsequence(&cand.chars, tok) {
        let s = 300u32.saturating_sub(gaps.saturating_mul(20)).max(10);
        return Some(TokenHit { kind: MatchKind::Subsequence, score: s, span: cand.byte_span(first..(last + 1)) });
    }

    // 6. Typo-tolerant fallback (bounded Damerau-Levenshtein on words of >= 5 non-digit chars).
    if !allow_typo {
        return None;
    }
    let max_dist = max_typo_distance(tok);
    if max_dist == 0 {
        return None;
    }
    let mut best_typo: Option<(usize, Range<usize>)> = None;
    for word in &cand.words {
        let Some(slice) = cand.chars.get(word.start..word.end) else { continue };
        if token_has_digit(slice) {
            continue;
        }
        if let Some(dist) = damerau_levenshtein(tok, slice, max_dist)
            && best_typo.as_ref().is_none_or(|(best_d, _)| dist < *best_d)
        {
            best_typo = Some((dist, word.start..word.end));
        }
    }
    // Also check whole candidate when it's a single short identifier/label without spaces.
    if cand.words.len() <= 1
        && !token_has_digit(&cand.chars)
        && let Some(dist) = damerau_levenshtein(tok, &cand.chars, max_dist)
        && best_typo.as_ref().is_none_or(|(best_d, _)| dist < *best_d)
    {
        best_typo = Some((dist, 0..cand.chars.len()));
    }
    best_typo.map(|(dist, range)| TokenHit {
        kind: MatchKind::Typo,
        score: 180u32.saturating_sub((dist as u32) * 60).max(20),
        span: cand.byte_span(range),
    })
}

fn find_subslice(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Match `needle` as an ordered subsequence of `haystack`.
/// Returns `(gap_count, first_index, last_index)`.
fn match_subsequence(haystack: &[char], needle: &[char]) -> Option<(u32, usize, usize)> {
    let mut gaps = 0u32;
    let mut first = None;
    let mut last = 0usize;
    let mut last_hit = true;
    let mut pos = 0usize;

    for &qc in needle {
        let mut found = false;
        while let Some(&c) = haystack.get(pos) {
            if c == qc {
                if first.is_none() {
                    first = Some(pos);
                }
                last = pos;
                pos += 1;
                found = true;
                break;
            }
            if last_hit && first.is_some() {
                gaps = gaps.saturating_add(1);
            }
            last_hit = false;
            pos += 1;
        }
        if !found {
            return None;
        }
        last_hit = true;
    }
    first.map(|f| (gaps, f, last))
}

/// Result of matching a query against an item's searchable fields.
#[derive(Clone, Debug)]
pub struct FieldMatch {
    pub confidence: MatchConfidence,
    pub match_kind: MatchKind,
    pub score: u32,
    pub near_match: bool,
    pub exact_identifier: bool,
    pub highlights: Vec<MatchSpan>,
}

/// Score a raw query against a single candidate string (non-typo), compatible with command
/// palette ranking: exact > prefix > word start > substring > ordered subsequence.
/// Returns `1` for an empty query and `0` for no match.
pub fn score_text(query: &str, candidate: &str) -> u32 {
    let q = fold_str(query.trim(), MAX_QUERY_CHARS);
    if q.is_empty() {
        return 1;
    }
    let cand = FoldedText::new(candidate);
    if cand.is_empty() {
        return 0;
    }
    let full: Vec<char> = q.chars().collect();
    if let Some(hit) = match_token(&full, &cand, true, false) {
        return hit.score;
    }
    // Multi-token fallback for phrases like "center rect" or "repeat last".
    let tokens: Vec<Vec<char>> = q.split_whitespace().take(MAX_QUERY_TOKENS).map(|t| t.chars().collect()).collect();
    if tokens.len() <= 1 {
        return 0;
    }
    let mut total = 0u32;
    for tok in &tokens {
        let Some(hit) = match_token(tok, &cand, true, false) else { return 0 };
        total = total.saturating_add(hit.score);
    }
    (total / tokens.len() as u32).saturating_sub(40).max(1)
}

/// Evaluate a query against an item's searchable fields, respecting `viewer` read permissions.
pub fn match_query_against_fields(raw_query: &str, fields: &[SearchField], viewer: &SearchViewer) -> Option<FieldMatch> {
    let trimmed = raw_query.trim();
    if trimmed.is_empty() {
        return Some(FieldMatch {
            confidence: MatchConfidence::Prefix,
            match_kind: MatchKind::Prefix,
            score: 1,
            near_match: false,
            exact_identifier: false,
            highlights: Vec::new(),
        });
    }
    let folded_query = fold_str(trimmed, MAX_QUERY_CHARS);
    let full_chars: Vec<char> = folded_query.chars().collect();
    let tokens: Vec<Vec<char>> =
        folded_query.split_whitespace().take(MAX_QUERY_TOKENS).filter(|t| !t.is_empty()).map(|t| t.chars().collect()).collect();
    if tokens.is_empty() {
        return None;
    }

    // Prepare readable fields once so we never match against fields the viewer cannot read.
    let readable: Vec<(&SearchField, FoldedText)> = fields
        .iter()
        .filter(|f| viewer.can_read(f.visibility) && !f.text.is_empty())
        .map(|f| (f, FoldedText::new(&f.text)))
        .filter(|(_, folded)| !folded.is_empty())
        .collect();
    if readable.is_empty() {
        return None;
    }

    // Pass 1: try matching the whole query string directly on any single readable field.
    let mut best_full: Option<(u32, MatchKind, bool, Option<MatchSpan>)> = None;
    for (field, folded) in &readable {
        let allow_subseq = field.is_title || field.is_identifier || field.weight >= 80;
        if let Some(hit) = match_token(&full_chars, folded, allow_subseq, true) {
            let weighted = hit.score.saturating_mul(field.weight) / 100;
            let exact_id = hit.kind == MatchKind::Exact && (field.is_identifier || field.is_title);
            let title_span = field.is_title.then_some(hit.span).flatten();
            let better = best_full.as_ref().is_none_or(|(best_s, best_k, _, _)| {
                (hit.kind != MatchKind::Typo && *best_k == MatchKind::Typo)
                    || ((hit.kind == MatchKind::Typo) == (*best_k == MatchKind::Typo) && weighted > *best_s)
            });
            if better {
                best_full = Some((weighted, hit.kind, exact_id, title_span));
            }
        }
    }

    if let Some((score, kind, exact_identifier, span)) = best_full
        && kind != MatchKind::Typo
    {
        let confidence = match kind {
            MatchKind::Exact => MatchConfidence::Exact,
            MatchKind::Prefix | MatchKind::WordStart | MatchKind::Substring => MatchConfidence::Prefix,
            MatchKind::Subsequence | MatchKind::Typo => MatchConfidence::Possible,
        };
        return Some(FieldMatch {
            confidence,
            match_kind: kind,
            score: score.saturating_add(120),
            near_match: false,
            exact_identifier,
            highlights: span.into_iter().collect(),
        });
    }

    // Pass 2: multi-token match across readable fields (every token must match at least one field).
    if tokens.len() > 1 {
        let mut total_score = 0u32;
        let mut worst_kind = MatchKind::Exact;
        let mut any_typo = false;
        let mut highlights = Vec::new();
        let mut matched_all = true;

        for tok in &tokens {
            let mut best_tok: Option<(u32, MatchKind, Option<MatchSpan>)> = None;
            for (field, folded) in &readable {
                let allow_subseq = field.is_title || field.is_identifier || field.weight >= 80;
                if let Some(hit) = match_token(tok, folded, allow_subseq, true) {
                    let weighted = hit.score.saturating_mul(field.weight) / 100;
                    let title_span = field.is_title.then_some(hit.span).flatten();
                    let better = best_tok.as_ref().is_none_or(|(best_s, best_k, _)| {
                        (hit.kind != MatchKind::Typo && *best_k == MatchKind::Typo)
                            || ((hit.kind == MatchKind::Typo) == (*best_k == MatchKind::Typo) && weighted > *best_s)
                    });
                    if better {
                        best_tok = Some((weighted, hit.kind, title_span));
                    }
                }
            }
            if let Some((s, k, span)) = best_tok {
                total_score = total_score.saturating_add(s);
                if k.rank() > worst_kind.rank() {
                    worst_kind = k;
                }
                if k == MatchKind::Typo {
                    any_typo = true;
                }
                if let Some(sp) = span {
                    highlights.push(sp);
                }
            } else {
                matched_all = false;
                break;
            }
        }

        if matched_all {
            let avg = total_score / tokens.len() as u32;
            let confidence = if any_typo || worst_kind == MatchKind::Subsequence { MatchConfidence::Possible } else { MatchConfidence::Prefix };
            return Some(FieldMatch {
                confidence,
                match_kind: worst_kind,
                score: avg,
                near_match: any_typo,
                exact_identifier: false,
                highlights: merge_spans(highlights),
            });
        }
    }

    // Pass 3: single-token typo fallback from Pass 1.
    best_full.map(|(score, kind, _, span)| FieldMatch {
        confidence: MatchConfidence::Possible,
        match_kind: kind,
        score,
        near_match: kind == MatchKind::Typo,
        exact_identifier: false,
        highlights: span.into_iter().collect(),
    })
}

/// Sort and merge overlapping/adjacent highlight spans so UI renderers can slice monotonically.
fn merge_spans(mut spans: Vec<MatchSpan>) -> Vec<MatchSpan> {
    if spans.len() <= 1 {
        return spans;
    }
    spans.sort_by_key(|s| (s.start, s.end));
    let mut merged: Vec<MatchSpan> = Vec::with_capacity(spans.len());
    for span in spans {
        if let Some(last) = merged.last_mut()
            && span.start <= last.end
        {
            last.end = last.end.max(span.end);
            continue;
        }
        merged.push(span);
    }
    merged
}

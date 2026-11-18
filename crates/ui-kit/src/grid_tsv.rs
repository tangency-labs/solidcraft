//! Tab-separated text as spreadsheets put it on the clipboard. Excel and Google Sheets write a
//! tab between cells and a line break between rows (Excel CRLF, Sheets LF), and wrap a cell in
//! double quotes only when it holds a tab, a line break or a quote, doubling the quotes inside.
//! Not a drawn element: the grid's clipboard format, kept apart so it is tested on its own.

/// The most cells a paste reads: a clipboard holding a whole worksheet must not stall a frame.
pub const MAX_PASTE_CELLS: usize = 100_000;

/// Write `rows` as tab-separated text: tabs between cells, CRLF between rows (none after the
/// last, as Google Sheets writes it), and quotes only around a cell that needs them.
pub fn write_tsv(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push_str("\r\n");
        }
        for (column, cell) in row.iter().enumerate() {
            if column > 0 {
                out.push('\t');
            }
            if cell.contains(['\t', '\n', '\r', '"']) {
                out.push('"');
                out.push_str(&cell.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(cell);
            }
        }
    }
    out
}

/// Read tab-separated text into rows of cells. Line breaks may be CRLF, LF or CR; a cell that
/// starts with a quote runs to its closing quote, so it may hold tabs and line breaks, with `""`
/// for a quote. One line break at the very end (Excel writes one) adds no empty row. Stops after
/// [`MAX_PASTE_CELLS`] cells.
pub fn read_tsv(text: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut cells = 0usize;
    let mut chars = text.chars().peekable();
    // Whether the cell being read started with a quote and its closing quote is still ahead.
    let mut quoted = false;
    let mut at_cell_start = true;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if at_cell_start => {
                quoted = true;
                at_cell_start = false;
            }
            '\t' => {
                row.push(std::mem::take(&mut cell));
                cells += 1;
                at_cell_start = true;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut cell));
                cells += 1;
                rows.push(std::mem::take(&mut row));
                at_cell_start = true;
            }
            _ => {
                cell.push(c);
                at_cell_start = false;
            }
        }
        if cells >= MAX_PASTE_CELLS {
            return rows;
        }
    }
    // The last row, unless the text ended with its line break.
    if !at_cell_start || !row.is_empty() || !cell.is_empty() || quoted {
        row.push(cell);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(cells: &[&[&str]]) -> Vec<Vec<String>> {
        cells.iter().map(|row| row.iter().map(|c| c.to_string()).collect()).collect()
    }

    #[test]
    fn plain_cells_round_trip_without_quotes() {
        let block = rows(&[&["width", "mm", "40 mm"], &["height", "mm", "width / 2"]]);
        let text = write_tsv(&block);
        assert_eq!(text, "width\tmm\t40 mm\r\nheight\tmm\twidth / 2");
        assert_eq!(read_tsv(&text), block);
    }

    #[test]
    fn cells_with_tabs_breaks_or_quotes_are_quoted_as_excel_writes_them() {
        let block = rows(&[&["a\tb", "two\nlines", "say \"hi\""], &["", "plain", ""]]);
        let text = write_tsv(&block);
        assert_eq!(text, "\"a\tb\"\t\"two\nlines\"\t\"say \"\"hi\"\"\"\r\n\tplain\t");
        assert_eq!(read_tsv(&text), block);
    }

    #[test]
    fn excel_and_sheets_clipboards_read_the_same() {
        // Excel: CRLF after every row, the last one too. Google Sheets: LF between rows only.
        let excel = "width\t40 mm\r\nheight\t20 mm\r\n";
        let sheets = "width\t40 mm\nheight\t20 mm";
        let expected = rows(&[&["width", "40 mm"], &["height", "20 mm"]]);
        assert_eq!(read_tsv(excel), expected);
        assert_eq!(read_tsv(sheets), expected);
        // A single cell, with Excel's trailing break.
        assert_eq!(read_tsv("25 mm\r\n"), rows(&[&["25 mm"]]));
        // An old Mac CR and a blank row in the middle.
        assert_eq!(read_tsv("a\rb\r\rc"), rows(&[&["a"], &["b"], &[""], &["c"]]));
        // A quote inside a cell that does not start with one is just a character.
        assert_eq!(read_tsv("5\" pipe\tx"), rows(&[&["5\" pipe", "x"]]));
    }

    #[test]
    fn hostile_clipboards_never_panic_and_stay_bounded() {
        for text in ["", "\"", "\"\"\"", "\t\t\r\n\r", "\"open\tcell\nnever closed", "\u{0}\u{FEFF}\t\u{10FFFF}"] {
            let _ = read_tsv(text);
        }
        let huge = "x\t".repeat(MAX_PASTE_CELLS * 2);
        let cells: usize = read_tsv(&huge).iter().map(Vec::len).sum();
        assert!(cells <= MAX_PASTE_CELLS + 1, "{cells}");
        assert_eq!(read_tsv(""), Vec::<Vec<String>>::new());
    }
}

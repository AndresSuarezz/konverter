use std::borrow::Cow;
use std::collections::HashSet;
use std::io::Cursor;

use calamine::{Data, Reader, Xlsx};
use rust_xlsxwriter::Workbook;
use serde_json::{Map, Number, Value};

use crate::convert::{ConvertOptions, Converter, Fidelity};
use crate::error::{Error, Result};
use crate::format::Format;

/// Conversiones entre CSV, XLSX y JSON.
///
/// Todas pasan por una tabla intermedia, así que cada formato solo necesita un lector y un
/// escritor. Limitaciones conocidas: de un XLSX solo se lee la primera hoja y se pierden
/// fórmulas y estilos (se conservan los valores); JSON debe ser un arreglo de objetos y los
/// valores anidados se escriben como texto JSON.
pub struct TabularConverter;

#[derive(Debug, Clone, PartialEq)]
enum Cell {
    Empty,
    Text(String),
    Number(f64),
    Bool(bool),
}

type Table = Vec<Vec<Cell>>;

impl Converter for TabularConverter {
    fn supports(&self, from: Format, to: Format) -> Option<Fidelity> {
        match (from, to) {
            (Format::Csv, Format::Xlsx)
            | (Format::Csv, Format::Json)
            | (Format::Json, Format::Csv)
            | (Format::Json, Format::Xlsx) => Some(Fidelity::Lossless),
            (Format::Xlsx, Format::Csv) | (Format::Xlsx, Format::Json) => Some(Fidelity::Lossy),
            _ => None,
        }
    }

    fn convert(
        &self,
        input: &[u8],
        from: Format,
        to: Format,
        options: &ConvertOptions,
    ) -> Result<Vec<u8>> {
        if self.supports(from, to).is_none() {
            return Err(Error::Unsupported { from, to });
        }
        let table = match from {
            Format::Csv => read_csv(input)?,
            Format::Xlsx => read_xlsx(input)?,
            _ => read_json(input)?,
        };
        match to {
            Format::Csv => write_csv(&table, options),
            Format::Xlsx => write_xlsx(&table),
            _ => write_json(&table),
        }
    }
}

fn decode_error(format: Format, message: impl ToString) -> Error {
    Error::Decode {
        format,
        message: message.to_string(),
    }
}

fn encode_error(format: Format, message: impl ToString) -> Error {
    Error::Encode {
        format,
        message: message.to_string(),
    }
}

// ---------------------------------------------------------------- CSV

fn read_csv(input: &[u8]) -> Result<Table> {
    let decoded = decode_text(input);
    let text = decoded.strip_prefix('\u{feff}').unwrap_or(&decoded);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(detect_delimiter(text))
        .from_reader(text.as_bytes());
    let mut table = Table::new();
    for record in reader.records() {
        let record = record.map_err(|e| decode_error(Format::Csv, e))?;
        table.push(record.iter().map(cell_from_text).collect());
    }
    Ok(table)
}

fn write_csv(table: &Table, options: &ConvertOptions) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    if options.csv_bom {
        out.extend_from_slice(b"\xEF\xBB\xBF");
    }
    {
        let mut writer = csv::WriterBuilder::new()
            .delimiter(options.csv_delimiter)
            .terminator(csv::Terminator::CRLF)
            .flexible(true)
            .from_writer(&mut out);
        for row in table {
            writer
                .write_record(row.iter().map(|cell| cell_text(cell).into_owned()))
                .map_err(|e| encode_error(Format::Csv, e))?;
        }
        writer.flush().map_err(|e| encode_error(Format::Csv, e))?;
    }
    Ok(out)
}

/// UTF-8 si es válido; si no, Windows-1252, que es lo que exporta Excel en español sin BOM.
pub(crate) fn decode_text(input: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(input) {
        Ok(text) => Cow::Borrowed(text),
        Err(_) => Cow::Owned(input.iter().map(|&b| windows_1252(b)).collect()),
    }
}

fn windows_1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9F => HIGH[usize::from(byte - 0x80)],
        _ => char::from(byte),
    }
}

/// Elige el separador de la primera línea contando solo fuera de comillas.
/// Excel en español usa `;`, así que no se puede asumir la coma.
fn detect_delimiter(text: &str) -> u8 {
    let first_line = text.lines().next().unwrap_or("");
    let mut in_quotes = false;
    let outside_quotes: Vec<u8> = first_line
        .bytes()
        .filter(|&byte| {
            if byte == b'"' {
                in_quotes = !in_quotes;
            }
            !in_quotes && byte != b'"'
        })
        .collect();
    // `max_by_key` devuelve el último máximo; al recorrer en orden inverso, en un empate
    // gana el candidato más a la izquierda (la coma).
    b",;\t|"
        .iter()
        .rev()
        .copied()
        .max_by_key(|&d| outside_quotes.iter().filter(|&&b| b == d).count())
        .unwrap_or(b',')
}

// --------------------------------------------------------------- XLSX

fn read_xlsx(input: &[u8]) -> Result<Table> {
    let mut workbook: Xlsx<_> =
        Xlsx::new(Cursor::new(input)).map_err(|e| decode_error(Format::Xlsx, e))?;
    let Some(range) = workbook.worksheet_range_at(0) else {
        return Ok(Table::new());
    };
    let range = range.map_err(|e| decode_error(Format::Xlsx, e))?;
    Ok(range
        .rows()
        .map(|row| row.iter().map(cell_from_data).collect())
        .collect())
}

fn cell_from_data(data: &Data) -> Cell {
    match data {
        Data::Empty => Cell::Empty,
        Data::String(s) if s.is_empty() => Cell::Empty,
        Data::String(s) => Cell::Text(s.clone()),
        Data::Int(i) => Cell::Number(*i as f64),
        Data::Float(f) => Cell::Number(*f),
        Data::Bool(b) => Cell::Bool(*b),
        Data::DateTime(dt) => Cell::Text(match dt.as_datetime() {
            Some(moment) if moment.format("%H%M%S").to_string() == "000000" => {
                moment.format("%Y-%m-%d").to_string()
            }
            Some(moment) => moment.format("%Y-%m-%dT%H:%M:%S").to_string(),
            None => dt.to_string(),
        }),
        Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::Text(s.clone()),
        Data::Error(e) => Cell::Text(e.to_string()),
    }
}

fn write_xlsx(table: &Table) -> Result<Vec<u8>> {
    let fail = |message: String| encode_error(Format::Xlsx, message);
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    for (r, row) in table.iter().enumerate() {
        let r = u32::try_from(r).map_err(|e| fail(e.to_string()))?;
        for (c, cell) in row.iter().enumerate() {
            let c = u16::try_from(c).map_err(|e| fail(e.to_string()))?;
            match cell {
                Cell::Empty => {}
                Cell::Text(s) => {
                    sheet
                        .write_string(r, c, s)
                        .map_err(|e| fail(e.to_string()))?;
                }
                Cell::Number(n) => {
                    sheet
                        .write_number(r, c, *n)
                        .map_err(|e| fail(e.to_string()))?;
                }
                Cell::Bool(b) => {
                    sheet
                        .write_boolean(r, c, *b)
                        .map_err(|e| fail(e.to_string()))?;
                }
            }
        }
    }
    workbook.save_to_buffer().map_err(|e| fail(e.to_string()))
}

// --------------------------------------------------------------- JSON

fn read_json(input: &[u8]) -> Result<Table> {
    let input = input.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(input);
    let value: Value = serde_json::from_slice(input).map_err(|e| decode_error(Format::Json, e))?;
    let Value::Array(items) = value else {
        return Err(decode_error(
            Format::Json,
            "se esperaba un arreglo de objetos",
        ));
    };
    let mut headers: Vec<&String> = Vec::new();
    let mut objects = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let Value::Object(map) = item else {
            return Err(decode_error(
                Format::Json,
                format!("el elemento {} no es un objeto", i + 1),
            ));
        };
        for key in map.keys() {
            if !headers.contains(&key) {
                headers.push(key);
            }
        }
        objects.push(map);
    }
    if objects.is_empty() {
        return Ok(Table::new());
    }
    let mut table = vec![headers.iter().map(|h| Cell::Text((*h).clone())).collect()];
    for map in objects {
        table.push(
            headers
                .iter()
                .map(|h| map.get(*h).map_or(Cell::Empty, cell_from_json))
                .collect(),
        );
    }
    Ok(table)
}

fn cell_from_json(value: &Value) -> Cell {
    match value {
        Value::Null => Cell::Empty,
        Value::Bool(b) => Cell::Bool(*b),
        Value::Number(n) => match n.as_f64() {
            // Enteros de más de 2^53 no caben en f64 sin perder dígitos: se conservan como texto.
            Some(f) if n.is_f64() || f.abs() < 9.0e15 => Cell::Number(f),
            _ => Cell::Text(n.to_string()),
        },
        Value::String(s) if s.is_empty() => Cell::Empty,
        Value::String(s) => Cell::Text(s.clone()),
        Value::Array(_) | Value::Object(_) => Cell::Text(value.to_string()),
    }
}

fn write_json(table: &Table) -> Result<Vec<u8>> {
    let Some((header_row, rows)) = table.split_first() else {
        return Ok(b"[]\n".to_vec());
    };
    let width = table.iter().map(Vec::len).max().unwrap_or(0);
    let headers = unique_headers(header_row, width);
    let items: Vec<Value> = rows
        .iter()
        .map(|row| {
            let mut object = Map::new();
            for (i, key) in headers.iter().enumerate() {
                object.insert(key.clone(), row.get(i).map_or(Value::Null, json_from_cell));
            }
            Value::Object(object)
        })
        .collect();
    let mut out = serde_json::to_vec_pretty(&items).map_err(|e| encode_error(Format::Json, e))?;
    out.push(b'\n');
    Ok(out)
}

fn json_from_cell(cell: &Cell) -> Value {
    match cell {
        Cell::Empty => Value::Null,
        Cell::Text(s) => Value::String(s.clone()),
        Cell::Bool(b) => Value::Bool(*b),
        Cell::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => Value::from(*n as i64),
        Cell::Number(n) => Number::from_f64(*n).map_or(Value::Null, Value::Number),
    }
}

/// Nombres de columna no vacíos y sin repetir (`columna`, `columna_2`...).
fn unique_headers(header_row: &[Cell], width: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    (0..width)
        .map(|i| {
            let base = header_row
                .get(i)
                .map(|c| cell_text(c).into_owned())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("column_{}", i + 1));
            let mut name = base.clone();
            let mut n = 2;
            while !seen.insert(name.clone()) {
                name = format!("{base}_{n}");
                n += 1;
            }
            name
        })
        .collect()
}

// ------------------------------------------------------------ celdas

fn cell_from_text(text: &str) -> Cell {
    if text.is_empty() {
        Cell::Empty
    } else if let Some(n) = parse_number(text) {
        Cell::Number(n)
    } else {
        Cell::Text(text.to_string())
    }
}

fn cell_text(cell: &Cell) -> Cow<'_, str> {
    match cell {
        Cell::Empty => Cow::Borrowed(""),
        Cell::Text(s) => Cow::Borrowed(s),
        Cell::Bool(b) => Cow::Borrowed(if *b { "true" } else { "false" }),
        Cell::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => {
            Cow::Owned((*n as i64).to_string())
        }
        Cell::Number(n) => Cow::Owned(n.to_string()),
    }
}

/// Convierte texto en número solo cuando hacerlo no puede dañar el dato: `007` o `0123456`
/// son códigos (se quedan como texto), y más de 15 dígitos no caben en un `f64`.
/// Solo se acepta el punto decimal; `1234,5` queda como texto.
fn parse_number(text: &str) -> Option<f64> {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (integer, fraction) = match unsigned.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (unsigned, None),
    };
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !all_digits(integer) || fraction.is_some_and(|f| !all_digits(f)) {
        return None;
    }
    if integer.len() > 1 && integer.starts_with('0') {
        return None;
    }
    if integer.len() + fraction.map_or(0, str::len) > 15 {
        return None;
    }
    let value: f64 = text.parse().ok()?;
    // "-0" no es un número útil: se conserva como texto.
    (value != 0.0 || !text.starts_with('-')).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_only_parsed_when_it_is_safe() {
        assert_eq!(parse_number("42"), Some(42.0));
        assert_eq!(parse_number("-3.5"), Some(-3.5));
        assert_eq!(parse_number("0.25"), Some(0.25));
        assert_eq!(parse_number("007"), None, "ceros a la izquierda = código");
        assert_eq!(parse_number("1234,5"), None);
        assert_eq!(parse_number("1e5"), None);
        assert_eq!(parse_number(".5"), None);
        assert_eq!(parse_number("5."), None);
        assert_eq!(parse_number("-0"), None);
        assert_eq!(parse_number("1234567890123456"), None, "16 dígitos");
        assert_eq!(parse_number(" 5"), None);
        assert_eq!(parse_number(""), None);
    }

    #[test]
    fn delimiter_is_detected_outside_quotes() {
        assert_eq!(detect_delimiter("a;b;c"), b';');
        assert_eq!(detect_delimiter("a,b,c"), b',');
        assert_eq!(detect_delimiter("a\tb"), b'\t');
        assert_eq!(detect_delimiter("\"x,y,z\";b"), b';');
        assert_eq!(detect_delimiter("solo"), b',');
    }

    #[test]
    fn invalid_utf8_falls_back_to_windows_1252() {
        // "Año €" en Windows-1252
        assert_eq!(decode_text(b"A\xF1o \x80"), "Año €");
        assert_eq!(decode_text("Año".as_bytes()), "Año");
    }

    #[test]
    fn json_headers_are_never_empty_or_repeated() {
        let header = vec![Cell::Text("a".into()), Cell::Text("a".into()), Cell::Empty];
        assert_eq!(
            unique_headers(&header, 4),
            ["a", "a_2", "column_3", "column_4"]
        );
    }

    #[test]
    fn json_must_be_an_array_of_objects() {
        assert!(matches!(
            read_json(br#"{"a":1}"#),
            Err(Error::Decode {
                format: Format::Json,
                ..
            })
        ));
        assert!(matches!(
            read_json(b"[1,2]"),
            Err(Error::Decode {
                format: Format::Json,
                ..
            })
        ));
    }

    #[test]
    fn huge_json_integers_stay_text() {
        let table = read_json(br#"[{"id":12345678901234567890}]"#).unwrap();
        assert_eq!(table[1][0], Cell::Text("12345678901234567890".into()));
    }
}

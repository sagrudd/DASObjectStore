use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

pub(crate) fn json_string(value: &Value, path: &[&str]) -> Option<String> {
    let value = json_path(value, path)?;
    if value.is_null() {
        None
    } else if let Some(text) = value.as_str() {
        Some(text.to_string())
    } else {
        Some(value.to_string())
    }
}

pub(crate) fn json_array_strings(value: &Value, path: &[&str]) -> Vec<String> {
    json_array(value, path)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}

pub(crate) fn json_u64(value: &Value, path: &[&str]) -> Option<u64> {
    json_path(value, path)?.as_u64()
}

pub(crate) fn json_f64(value: &Value, path: &[&str]) -> Option<f64> {
    json_path(value, path)?.as_f64()
}

pub(crate) fn json_bool(value: &Value, path: &[&str]) -> Option<bool> {
    json_path(value, path)?.as_bool()
}

pub(crate) fn json_array<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Vec<Value>> {
    json_path(value, path)?.as_array()
}

pub(crate) fn json_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

pub(crate) fn hostname_for_report() -> String {
    ProcessCommand::new("hostname")
        .output()
        .ok()
        .and_then(|output| {
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "not recorded".to_string())
}

pub(crate) fn sha256_hex_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

#[cfg(test)]
pub(crate) fn render_simple_pdf(markdown: &str) -> Vec<u8> {
    let lines = markdown
        .lines()
        .map(strip_markdown_for_pdf)
        .collect::<Vec<_>>();
    let lines_per_page = 48_usize;
    let page_count = lines.len().div_ceil(lines_per_page).max(1);
    let font_id = 3 + page_count * 2;
    let mut objects = Vec::<String>::new();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());
    let kids = (0..page_count)
        .map(|index| format!("{} 0 R", 3 + index * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!(
        "<< /Type /Pages /Kids [{kids}] /Count {page_count} >>"
    ));
    for page_index in 0..page_count {
        let page_id = 3 + page_index * 2;
        let content_id = page_id + 1;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font_id} 0 R >> >> /Contents {content_id} 0 R >>"
        ));
        let page_lines = lines
            .iter()
            .skip(page_index * lines_per_page)
            .take(lines_per_page)
            .collect::<Vec<_>>();
        let mut stream = String::from("BT /F1 9 Tf 36 756 Td 0 -14 Td\n");
        for line in page_lines {
            stream.push_str(&format!("({}) Tj 0 -14 Td\n", escape_pdf_text(line)));
        }
        stream.push_str("ET");
        objects.push(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        ));
    }
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());

    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", index + 1, object));
    }
    let xref_start = pdf.len();
    pdf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref_start}\n%%EOF\n",
        objects.len() + 1
    ));
    pdf.into_bytes()
}

#[cfg(test)]
pub(crate) fn strip_markdown_for_pdf(line: &str) -> String {
    line.replace("**", "")
        .replace('`', "")
        .replace("<br>", " | ")
        .chars()
        .take(110)
        .collect()
}

#[cfg(test)]
pub(crate) fn escape_pdf_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

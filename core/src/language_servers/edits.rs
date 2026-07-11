use super::{LanguageServerError, LanguageServerPosition, LanguageServerTextEdit};

pub(super) fn validate_text_edits(
    text: &str,
    edits: &[LanguageServerTextEdit],
) -> Result<(), LanguageServerError> {
    let spans = edits
        .iter()
        .enumerate()
        .map(|(index, edit)| {
            let start = position_offset(text, edit.range.start)?;
            let end = position_offset(text, edit.range.end)?;
            if start > end {
                return Err(invalid_edits("formatting edit range is reversed"));
            }
            Ok((start, end, index))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (index, &(start, end, _)) in spans.iter().enumerate() {
        for &(other_start, other_end, _) in &spans[index + 1..] {
            let overlaps = match (start == end, other_start == other_end) {
                (true, true) => false,
                (true, false) => start > other_start && start < other_end,
                (false, true) => other_start > start && other_start < end,
                (false, false) => start.max(other_start) < end.min(other_end),
            };
            if overlaps {
                return Err(invalid_edits("formatting edits overlap"));
            }
        }
    }
    Ok(())
}

fn position_offset(
    text: &str,
    position: LanguageServerPosition,
) -> Result<usize, LanguageServerError> {
    let mut line_starts = vec![0];
    line_starts.extend(
        text.match_indices('\n')
            .map(|(offset, _)| offset.saturating_add(1)),
    );
    let line = usize::try_from(position.line)
        .ok()
        .and_then(|line| line_starts.get(line).copied())
        .ok_or_else(|| invalid_edits("formatting edit line is outside the document"))?;
    let mut line_end = line_starts
        .get(
            usize::try_from(position.line)
                .unwrap_or(usize::MAX)
                .saturating_add(1),
        )
        .map_or(text.len(), |next| next.saturating_sub(1));
    if text.as_bytes().get(line_end.saturating_sub(1)) == Some(&b'\r') {
        line_end = line_end.saturating_sub(1);
    }
    let content = &text[line..line_end];
    let target = usize::try_from(position.character)
        .map_err(|_| invalid_edits("formatting edit character is invalid"))?;
    let mut utf16 = 0;
    for (offset, character) in content.char_indices() {
        if utf16 == target {
            return Ok(line + offset);
        }
        utf16 += character.len_utf16();
        if utf16 > target {
            return Err(invalid_edits(
                "formatting edit splits a UTF-16 surrogate pair",
            ));
        }
    }
    if utf16 == target {
        Ok(line_end)
    } else {
        Err(invalid_edits(
            "formatting edit character is outside the line",
        ))
    }
}

fn invalid_edits(message: &str) -> LanguageServerError {
    LanguageServerError::Protocol(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language_servers::LanguageServerRange;

    fn edit(start: (u32, u32), end: (u32, u32)) -> LanguageServerTextEdit {
        LanguageServerTextEdit {
            range: LanguageServerRange {
                start: LanguageServerPosition {
                    line: start.0,
                    character: start.1,
                },
                end: LanguageServerPosition {
                    line: end.0,
                    character: end.1,
                },
            },
            new_text: String::new(),
        }
    }

    #[test]
    fn accepts_adjacent_out_of_order_edits() {
        let edits = [
            edit((0, 2), (0, 4)),
            edit((0, 0), (0, 2)),
            edit((0, 2), (0, 2)),
            edit((0, 2), (0, 2)),
        ];

        assert!(validate_text_edits("text", &edits).is_ok());
    }

    #[test]
    fn rejects_overlapping_and_reversed_edits() {
        assert!(
            validate_text_edits("text", &[edit((0, 0), (0, 3)), edit((0, 2), (0, 4))]).is_err()
        );
        assert!(validate_text_edits("text", &[edit((0, 3), (0, 1))]).is_err());
    }

    #[test]
    fn validates_utf16_and_crlf_positions() {
        assert!(validate_text_edits("a😀b\r\n", &[edit((0, 1), (0, 3))]).is_ok());
        assert!(validate_text_edits("a😀b\r\n", &[edit((0, 2), (0, 3))]).is_err());
        assert!(validate_text_edits("a😀b\r\n", &[edit((1, 0), (1, 0))]).is_ok());
        assert!(validate_text_edits("a😀b\r\n", &[edit((0, 4), (0, 5))]).is_err());
    }
}

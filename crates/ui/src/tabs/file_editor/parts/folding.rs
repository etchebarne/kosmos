#[cfg(test)]
fn indent_guide_runs(rows: &[(usize, &[usize])]) -> Vec<(usize, usize, usize)> {
    let mut active: Vec<(usize, usize)> = Vec::new();
    let mut runs = Vec::new();
    for (row_index, guides) in rows {
        let mut index = 0;
        while index < active.len() {
            if guides.binary_search(&active[index].0).is_ok() {
                index += 1;
            } else {
                let (column, start) = active.remove(index);
                runs.push((column, start, *row_index));
            }
        }
        for &column in *guides {
            if active
                .iter()
                .all(|(active_column, _)| *active_column != column)
            {
                active.push((column, *row_index));
            }
        }
    }
    let end = rows.last().map(|(index, _)| index + 1).unwrap_or(0);
    runs.extend(
        active
            .into_iter()
            .map(|(column, start)| (column, start, end)),
    );
    runs.sort_unstable();
    runs
}

fn indents_for_buffer(buffer: &Buffer) -> Vec<Option<usize>> {
    (0..buffer.line_count())
        .map(|index| buffer.line(index).and_then(indentation_columns))
        .collect::<Vec<_>>()
}

fn foldable_lines_for_indents(indents: &[Option<usize>]) -> Vec<bool> {
    (0..indents.len())
        .map(|index| {
            let Some(indent) = indents[index] else {
                return false;
            };
            indents[index + 1..]
                .iter()
                .flatten()
                .next()
                .is_some_and(|next| *next > indent)
        })
        .collect()
}

fn visible_lines_for_indents(
    indents: &[Option<usize>],
    foldable_lines: &[bool],
    folded_lines: &HashSet<usize>,
) -> Vec<usize> {
    let mut visible = Vec::with_capacity(indents.len());
    let mut index = 0usize;
    while index < indents.len() {
        visible.push(index);
        if folded_lines.contains(&index) && foldable_lines.get(index).copied().unwrap_or(false) {
            index = fold_end_for_indents(index, indents);
        } else {
            index += 1;
        }
    }
    visible
}

fn fold_end_for_indents(start: usize, indents: &[Option<usize>]) -> usize {
    let Some(indent) = indents.get(start).copied().flatten() else {
        return start.saturating_add(1).min(indents.len());
    };
    let mut end = start + 1;
    while end < indents.len() {
        if let Some(next_indent) = indents[end]
            && next_indent <= indent
        {
            break;
        }
        end += 1;
    }
    end
}

fn longest_visible_row_index(buffer: &Buffer, visible_lines: &[usize]) -> usize {
    visible_lines
        .iter()
        .enumerate()
        .max_by_key(|(_, line_index)| buffer.line_chars(**line_index))
        .map(|(row_index, _)| row_index)
        .unwrap_or(0)
}

fn indentation_columns(line: &str) -> Option<usize> {
    let mut columns = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => columns += 1,
            '\t' => columns += TAB_SIZE_COLUMNS - (columns % TAB_SIZE_COLUMNS),
            _ => break,
        }
    }

    line.chars()
        .any(|ch| !ch.is_whitespace())
        .then_some(columns)
}

fn indent_guides_for_indents(indents: &[Option<usize>]) -> Vec<Vec<usize>> {
    let indent_width = infer_indent_width(indents);
    (0..indents.len())
        .map(|index| {
            let columns = effective_indent_columns(index, indents);
            indent_guide_columns(columns, indent_width)
        })
        .collect()
}

fn infer_indent_width(indents: &[Option<usize>]) -> usize {
    let mut counts = [0usize; 9];
    let mut previous = None;
    for indent in indents.iter().flatten().copied() {
        if let Some(previous) = previous
            && indent > previous
        {
            let delta = indent - previous;
            if (2..counts.len()).contains(&delta) {
                counts[delta] += 1;
            }
        }
        previous = Some(indent);
    }

    if let Some((width, _)) = counts
        .iter()
        .enumerate()
        .skip(2)
        .max_by_key(|(width, count)| (**count, *width))
        .filter(|(_, count)| **count > 0)
    {
        return width;
    }

    indents
        .iter()
        .flatten()
        .copied()
        .find(|indent| (2..=8).contains(indent))
        .unwrap_or(DEFAULT_INDENT_GUIDE_COLUMNS)
}

fn effective_indent_columns(index: usize, indents: &[Option<usize>]) -> usize {
    if let Some(indent) = indents[index] {
        return indent;
    }

    let previous = indents[..index].iter().rev().flatten().next().copied();
    let next = indents[index + 1..].iter().flatten().next().copied();
    match (previous, next) {
        (Some(previous), Some(next)) => previous.min(next),
        _ => 0,
    }
}

fn indent_guide_columns(columns: usize, indent_width: usize) -> Vec<usize> {
    if indent_width == 0 {
        return Vec::new();
    }

    let offset = indent_width;
    (indent_width..=columns)
        .step_by(indent_width)
        .map(|column| column.saturating_sub(offset))
        .collect()
}

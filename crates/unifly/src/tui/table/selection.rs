//! Keeping a table's selection on the same entity when its rows change.

/// Row to select after a table's rows changed.
///
/// `keys` are the identities of the new rows in display order. The
/// selection follows `selected` (the identity of the entity selected
/// before) to its new row when it is still listed, and otherwise stays at
/// the `previous` row index clamped to the new length. Returns `None` only
/// when there are no rows.
pub fn reselect<'a, K>(
    keys: impl IntoIterator<Item = &'a K>,
    selected: Option<&K>,
    previous: Option<usize>,
) -> Option<usize>
where
    K: PartialEq + ?Sized + 'a,
{
    let mut len = 0;
    let mut found = None;
    for (index, key) in keys.into_iter().enumerate() {
        if found.is_none() && selected == Some(key) {
            found = Some(index);
        }
        len = index + 1;
    }

    if len == 0 {
        return None;
    }
    found.or_else(|| Some(previous.unwrap_or(0).min(len - 1)))
}

#[cfg(test)]
mod tests {
    use super::reselect;

    #[test]
    fn follows_the_selected_key_to_its_new_row() {
        let rows = ["delta", "alpha", "charlie"];
        assert_eq!(reselect(rows, Some("charlie"), Some(0)), Some(2));
    }

    #[test]
    fn keeps_the_row_index_when_the_selected_key_is_gone() {
        let rows = ["alpha", "bravo", "charlie"];
        assert_eq!(reselect(rows, Some("zulu"), Some(1)), Some(1));
        assert_eq!(reselect(rows, Some("zulu"), Some(9)), Some(2));
        assert_eq!(reselect(rows, None, None), Some(0));
    }

    #[test]
    fn empty_tables_select_nothing() {
        let rows: [&str; 0] = [];
        assert_eq!(reselect(rows, Some("alpha"), Some(3)), None);
    }
}

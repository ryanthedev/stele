//! DW-4.1 through DW-4.10 for the editable buffer — the only code in this
//! workspace that can destroy a reader's data.
//!
//! **Every assertion about what happened is made against the disk**, through
//! [`snapshot`], which reads every name in the directory and everything
//! stored under it. Nothing here trusts the plan's own account of itself: a
//! diff that reported the right operations and applied the wrong ones would
//! pass a plan-shaped assertion and fail every one below. The plan *is*
//! asserted on in places, but always alongside the disk, never instead of it.
//!
//! Fixtures are hand-rolled from `std::env::temp_dir()` and `process::id()`,
//! the idiom `explore_listing.rs` and `link_interaction.rs` already use —
//! there is no `tempfile` in this workspace.

#![cfg(unix)]

mod common;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use ast::Document;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use layout::{LayoutConfig, NullSizer, layout};
use stele::app::{AppState, FileInfo, PendingAction, StatusMessage};
use stele::explore::{
    EditBuffer, EditError, EditPlan, Entry, EntryKind, Listing, NameFault, Operation, RowStyle,
    apply_summary,
};
use stele::painter::Size;
use width::{WidthConfig, WidthEngine};

// ------------------------------------------------------------------ fixtures

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("stele-edit-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Four files and a subdirectory, whose sorted listing is exactly
/// `../`, `alpha.md`, `beta.md`, `gamma.md`, `sub/` — so an entry index in a
/// buffer line is readable straight off this comment.
fn fixture(tag: &str) -> PathBuf {
    let dir = scratch_dir(tag);
    fs::write(dir.join("alpha.md"), "alpha-contents\n").expect("write alpha");
    fs::write(dir.join("beta.md"), "beta-contents\n").expect("write beta");
    fs::write(dir.join("gamma.md"), "gamma-contents\n").expect("write gamma");
    fs::create_dir(dir.join("sub")).expect("create sub");
    fs::write(dir.join("sub").join("inner.md"), "inner-contents\n").expect("write inner");
    dir
}

/// Everything in `dir`, name by name, with what is actually stored under it.
///
/// **The oracle.** A directory is rendered as its own sorted child names, so
/// a recursive delete — the thing this phase must never do — changes the
/// snapshot even though the directory itself survives. A symlink is rendered
/// as its target without following it, so unlinking a link and deleting what
/// it pointed at are different snapshots.
fn snapshot(dir: &Path) -> BTreeMap<OsString, String> {
    let mut out = BTreeMap::new();
    for entry in fs::read_dir(dir).expect("read directory for snapshot") {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).expect("stat for snapshot");
        let value = if meta.file_type().is_symlink() {
            format!(
                "<symlink to {}>",
                fs::read_link(&path).expect("read link").display()
            )
        } else if meta.is_dir() {
            let mut names: Vec<String> = fs::read_dir(&path)
                .expect("read subdirectory")
                .map(|child| {
                    child
                        .expect("child entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            names.sort();
            format!("<dir [{}]>", names.join(", "))
        } else {
            fs::read(&path)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_else(|error| format!("<unreadable: {error}>"))
        };
        out.insert(entry.file_name(), value);
    }
    out
}

// -------------------------------------------------------- buffer-line helpers

/// The buffer lines a reader would be handed for `listing`.
fn lines_of(listing: &Listing) -> Vec<String> {
    EditBuffer::new(listing).lines()
}

/// Where the line for `name` is, so a test can say "rename alpha.md" instead
/// of "edit line 0".
fn line_for(lines: &[String], name: &str) -> usize {
    lines
        .iter()
        .position(|line| line.split_once('\t').map(|(_, text)| text) == Some(name))
        .unwrap_or_else(|| panic!("no buffer line for `{name}` in {lines:?}"))
}

fn rename_line(lines: &mut [String], from: &str, to: &str) {
    rename_lines(lines, &[(from, to)]);
}

/// Several renames at once, with **every line located before any is
/// changed**.
///
/// Not a convenience. Applying two renames one at a time by searching for the
/// current text is how the cycle test silently stopped testing a cycle: after
/// `a.md` → `b.md`, searching for `b.md` finds the line just edited rather
/// than `b.md`'s own, and `b.md` → `a.md` merely undoes the first edit. The
/// diff then correctly reported an empty plan and the assertion for a refusal
/// failed — which is the only reason this was caught rather than shipped as a
/// green test proving nothing.
fn rename_lines(lines: &mut [String], edits: &[(&str, &str)]) {
    let located: Vec<usize> = edits
        .iter()
        .map(|(from, _)| line_for(lines, from))
        .collect();
    for (index, (_, to)) in located.into_iter().zip(edits) {
        let (id, _) = lines[index].split_once('\t').expect("a line carries an id");
        lines[index] = format!("{id}\t{to}");
    }
}

fn remove_line(lines: &mut Vec<String>, name: &str) {
    let index = line_for(lines, name);
    lines.remove(index);
}

fn add_line(lines: &mut Vec<String>, name: &str) {
    lines.push(format!("\t{name}"));
}

fn diff(listing: &Listing, lines: &[String]) -> Result<EditPlan, EditError> {
    EditPlan::diff(listing, lines)
}

/// Applies `plan` and asserts every single operation succeeded, returning
/// nothing — the caller's next line is a disk assertion, which is the real
/// check.
fn apply_expecting_success(plan: &EditPlan) {
    for (op, result) in plan.apply() {
        assert!(result.is_ok(), "{op} failed: {:?}", result.err());
    }
}

// ------------------------------------------------------------------- DW-4.1

/// Editing one row's text renames exactly that file and touches nothing else.
///
/// The oracle is the whole directory before and after: the expected snapshot
/// is built by *transforming* the "before" map (drop one key, add one with
/// the identical contents), so any extra creation, any lost byte, and any
/// collateral rename fails the comparison. Trusting `plan.operations()` here
/// would prove only that the diff computed what it computed.
#[test]
fn test_dw_4_1_editing_one_row_renames_only_that_file_on_disk() {
    let dir = fixture("dw41-rename");
    let before = snapshot(&dir);

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "alpha.md", "renamed.md");

    let plan = diff(&listing, &lines).expect("a plain rename is a legal edit");
    apply_expecting_success(&plan);

    let mut expected = before.clone();
    let contents = expected
        .remove(OsString::from("alpha.md").as_os_str())
        .expect("alpha.md was there before");
    expected.insert(OsString::from("renamed.md"), contents);
    assert_eq!(snapshot(&dir), expected);
}

/// A directory row paints as `name/`. Renaming one keeps it a directory and
/// keeps everything inside it — the snapshot renders a directory as its own
/// children, so a rename that lost `inner.md` fails here.
#[test]
fn test_dw_4_1_renaming_a_directory_row_keeps_everything_inside_it() {
    let dir = fixture("dw41-dir");
    let before = snapshot(&dir);

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "sub/", "moved/");

    let plan = diff(&listing, &lines).expect("renaming a directory is a legal edit");
    apply_expecting_success(&plan);

    let mut expected = before.clone();
    let children = expected
        .remove(OsString::from("sub").as_os_str())
        .expect("sub was there before");
    expected.insert(OsString::from("moved"), children);
    assert_eq!(snapshot(&dir), expected);
    assert_eq!(
        fs::read_to_string(dir.join("moved").join("inner.md")).expect("inner survived"),
        "inner-contents\n"
    );
}

/// Dropping a directory row's trailing slash is not a rename. The slash is
/// display sugar; without this the first reader to tidy `sub/` into `sub`
/// would be asking to rename `sub` to `sub`.
#[test]
fn test_dropping_a_directory_rows_trailing_slash_is_not_a_rename() {
    let dir = fixture("slash-drop");
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "sub/", "sub");

    let plan = diff(&listing, &lines).expect("legal");
    assert!(
        plan.is_empty(),
        "expected no operations, got {:?}",
        plan.operations()
    );
}

// ------------------------------------------------------------------- DW-4.2

/// Removing a row deletes exactly that file.
#[test]
fn test_dw_4_2_a_removed_row_deletes_only_that_file_on_disk() {
    let dir = fixture("dw42-delete");
    let before = snapshot(&dir);

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    remove_line(&mut lines, "beta.md");

    let plan = diff(&listing, &lines).expect("removing a row is a legal edit");
    assert_eq!(plan.destructive().len(), 1, "{:?}", plan.operations());
    apply_expecting_success(&plan);

    let mut expected = before.clone();
    expected.remove(OsString::from("beta.md").as_os_str());
    assert_eq!(snapshot(&dir), expected);
}

/// Adding a row creates an empty file of that name.
#[test]
fn test_dw_4_2_an_added_row_creates_an_empty_file() {
    let dir = fixture("dw42-create");
    let before = snapshot(&dir);

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    add_line(&mut lines, "fresh.md");

    let plan = diff(&listing, &lines).expect("adding a row is a legal edit");
    apply_expecting_success(&plan);

    let mut expected = before.clone();
    expected.insert(OsString::from("fresh.md"), String::new());
    assert_eq!(snapshot(&dir), expected);
}

/// A collider that appears **after** the diff refuses the whole plan.
///
/// **Renamed, because the old name was a lie a mutation sweep caught.** This
/// was called `…_a_create_never_truncates_…` and tagged DW-4.2, and it tests
/// no such thing: the pre-flight refuses before `create_new` is ever reached,
/// so weakening the open to `.create(true).truncate(true)` left this green.
/// What it actually pins is the pre-flight — a real and separate guarantee,
/// worth keeping under an honest name. The `create_new` guarantee is pinned
/// where it can be reached, by
/// `explore::tests::test_create_exclusively_refuses_an_existing_file_and_keeps_its_bytes`.
#[test]
fn test_dw_4_5_a_create_whose_name_was_taken_after_the_diff_refuses_the_plan() {
    let dir = fixture("dw42-squat");

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    add_line(&mut lines, "fresh.md");
    let plan = diff(&listing, &lines).expect("legal at diff time");

    fs::write(dir.join("fresh.md"), "SQUATTER\n").expect("write the collider");
    let before = snapshot(&dir);

    let results = plan.apply();
    assert!(
        results.iter().all(|(_, result)| result.is_err()),
        "a collision must refuse the whole plan: {results:?}"
    );
    assert_eq!(snapshot(&dir), before, "nothing may have changed");
    assert_eq!(
        fs::read_to_string(dir.join("fresh.md")).expect("the squatter survived"),
        "SQUATTER\n"
    );
}

/// Storage order is apply order: renames, then creates, then deletes — and
/// `destructive()` is exactly the delete tail, not a filtered copy that could
/// disagree with it.
#[test]
fn test_dw_4_2_the_plan_orders_renames_and_creates_before_every_delete() {
    let dir = fixture("dw42-order");
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "alpha.md", "renamed.md");
    add_line(&mut lines, "fresh.md");
    remove_line(&mut lines, "gamma.md");

    let plan = diff(&listing, &lines).expect("legal");
    let ops = plan.operations();
    assert_eq!(ops.len(), 3, "{ops:?}");
    assert!(matches!(ops[0], Operation::Rename { .. }), "{ops:?}");
    assert!(matches!(ops[1], Operation::Create { .. }), "{ops:?}");
    assert!(matches!(ops[2], Operation::Delete { .. }), "{ops:?}");
    assert_eq!(plan.destructive(), &ops[2..], "{ops:?}");

    // And behaviourally: all three land. A plan whose deletes were ordered
    // into the rename phase would hit `apply`'s internal-error arm instead.
    apply_expecting_success(&plan);
    let after = snapshot(&dir);
    assert!(after.contains_key(OsString::from("renamed.md").as_os_str()));
    assert!(after.contains_key(OsString::from("fresh.md").as_os_str()));
    assert!(!after.contains_key(OsString::from("gamma.md").as_os_str()));
    assert!(!after.contains_key(OsString::from("alpha.md").as_os_str()));
}

// ------------------------------------------------------------------- DW-4.3

fn app_state() -> AppState {
    let doc = Document::parse("placeholder\n");
    let engine = WidthEngine::new(WidthConfig::default());
    let tree = layout(&doc, 80, &LayoutConfig::default(), &engine, &NullSizer);
    AppState::new(
        tree,
        Size {
            width: 80,
            height: 24,
        },
        FileInfo::default(),
    )
}

fn press(state: &mut AppState, code: KeyCode) {
    state.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
}

fn press_char(state: &mut AppState, ch: char) {
    press(state, KeyCode::Char(ch));
}

/// Clears the row being typed into and types `text` in its place.
fn retype(state: &mut AppState, text: &str) {
    for _ in 0..64 {
        press(state, KeyCode::Backspace);
    }
    for ch in text.chars() {
        press_char(state, ch);
    }
    press(state, KeyCode::Enter);
}

fn status_text(state: &mut AppState) -> String {
    state.status().message.unwrap_or_default()
}

/// The gate, end to end: an edit naming a rename, a create and a delete
/// together, and a `n` that leaves the directory byte-for-byte as it was.
///
/// Then the same buffer confirmed, which proves the first half was a gate and
/// not merely a slow path: pressing `y` queues the plan and **still** touches
/// nothing, because `AppState` cannot write. Only the explicit `apply` — the
/// call `main.rs` makes on that queued action — changes the disk.
#[test]
fn test_dw_4_3_a_rejected_confirmation_runs_no_operation_at_all() {
    let dir = fixture("dw43-gate");
    let before = snapshot(&dir);

    let mut state = app_state();
    // Entries sort to `../`, alpha.md, beta.md, gamma.md, sub/ — so row 1 is
    // alpha.md and the buffer holds one row per entry in the same order.
    state.install_listing(Listing::read(&dir), 1, true);

    press_char(&mut state, 'i');
    retype(&mut state, "renamed.md");
    press_char(&mut state, 'j'); // beta.md
    press_char(&mut state, 'd'); // mark it for removal
    press_char(&mut state, 'o'); // a new row, already being typed into
    retype(&mut state, "created.md");

    press_char(&mut state, 'w');
    let question = status_text(&mut state);
    assert!(
        question.contains("DELETE 1 (beta.md)") && question.contains("[y/n]"),
        "the gate must name the doomed file: {question}"
    );
    assert!(
        state.take_action().is_none(),
        "building the plan must queue nothing"
    );
    assert_eq!(snapshot(&dir), before, "no write may have happened yet");

    press_char(&mut state, 'n');
    assert!(
        state.take_action().is_none(),
        "a rejected confirmation must queue nothing"
    );
    assert!(
        status_text(&mut state).contains("nothing on disk was changed"),
        "the reader must be told the refusal was total"
    );
    assert_eq!(
        snapshot(&dir),
        before,
        "a rejected confirmation must leave the directory byte-for-byte unchanged"
    );

    // The same buffer, accepted this time.
    press_char(&mut state, 'w');
    press_char(&mut state, 'y');
    let Some(PendingAction::ApplyEdits(plan)) = state.take_action() else {
        panic!("`y` must queue the plan");
    };
    assert_eq!(
        snapshot(&dir),
        before,
        "even an accepted confirmation writes nothing inside AppState"
    );

    apply_expecting_success(&plan);
    let after = snapshot(&dir);
    assert!(after.contains_key(OsString::from("renamed.md").as_os_str()));
    assert!(after.contains_key(OsString::from("created.md").as_os_str()));
    assert!(!after.contains_key(OsString::from("beta.md").as_os_str()));
    assert!(!after.contains_key(OsString::from("alpha.md").as_os_str()));
}

/// The gate is a gate against the *keyboard*, not only against `n`. `Enter`
/// is the key a reader presses without reading, so it must not confirm.
#[test]
fn test_dw_4_3_only_y_confirms_and_every_other_key_is_inert() {
    let dir = fixture("dw43-keys");
    let before = snapshot(&dir);

    let mut state = app_state();
    state.install_listing(Listing::read(&dir), 1, true);
    press_char(&mut state, 'd'); // mark alpha.md for removal
    press_char(&mut state, 'w');
    assert!(status_text(&mut state).contains("DELETE 1 (alpha.md)"));

    for code in [
        KeyCode::Enter,
        KeyCode::Char('Y'),
        KeyCode::Char('d'),
        KeyCode::Char('j'),
        KeyCode::Char('w'),
        KeyCode::Char('a'),
        KeyCode::Backspace,
        KeyCode::Down,
    ] {
        press(&mut state, code);
        assert!(
            state.take_action().is_none(),
            "{code:?} must not confirm a delete"
        );
        assert!(
            status_text(&mut state).contains("[y/n]"),
            "{code:?} must leave the question up"
        );
        assert_eq!(snapshot(&dir), before);
    }

    press_char(&mut state, 'y');
    assert!(
        matches!(state.take_action(), Some(PendingAction::ApplyEdits(_))),
        "`y` is the one key that confirms"
    );
}

/// The plan is inert on its own. Holding one — building it, cloning it,
/// reading it, describing it — must never touch the filesystem; only `apply`
/// may.
#[test]
fn test_dw_4_3_the_plan_alone_touches_nothing_until_apply_is_called() {
    let dir = fixture("dw43-inert");
    let before = snapshot(&dir);

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "alpha.md", "renamed.md");
    remove_line(&mut lines, "beta.md");
    add_line(&mut lines, "created.md");

    let plan = diff(&listing, &lines).expect("legal");
    let _ = plan.confirmation();
    let _ = plan.destructive();
    let _ = plan.clone();
    let _ = format!("{plan:?}");
    assert_eq!(snapshot(&dir), before, "the plan is a value, not an action");

    apply_expecting_success(&plan);
    assert_ne!(snapshot(&dir), before, "and `apply` is the action");
}

// ------------------------------------------------------------------- DW-4.4

/// Position is not identity: a buffer whose rows were shuffled but whose
/// texts are untouched asks for nothing.
#[test]
fn test_dw_4_4_reordering_every_row_produces_an_empty_plan() {
    let dir = fixture("dw44-reorder");
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    lines.rotate_left(2);

    let plan = diff(&listing, &lines).expect("a reorder is legal");
    assert!(
        plan.is_empty(),
        "a reordered buffer must ask for nothing: {:?}",
        plan.operations()
    );
    assert_eq!(plan.suppressed_deletes(), 0);
}

/// The strongest form of the same claim, and the one that actually catches a
/// position-based diff: a fully reversed buffer, where no row is where it was.
#[test]
fn test_dw_4_4_a_reversed_buffer_is_still_an_empty_plan() {
    let dir = fixture("dw44-reverse");
    let before = snapshot(&dir);
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    lines.reverse();

    let plan = diff(&listing, &lines).expect("a reversal is legal");
    assert!(plan.is_empty(), "{:?}", plan.operations());
    apply_expecting_success(&plan);
    assert_eq!(snapshot(&dir), before);
}

// ------------------------------------------------------------------- DW-4.5

/// A rename onto a name the listing already holds is refused at diff time,
/// before a plan exists at all.
#[test]
fn test_dw_4_5_a_rename_onto_a_listed_name_is_refused_at_diff_time() {
    let dir = fixture("dw45-listed");
    let before = snapshot(&dir);
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "alpha.md", "beta.md");

    match diff(&listing, &lines) {
        Err(EditError::NameTaken { name, .. }) => assert_eq!(name, "beta.md"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(snapshot(&dir), before);
}

/// The same refusal for a create.
#[test]
fn test_dw_4_5_a_create_of_a_listed_name_is_refused_at_diff_time() {
    let dir = fixture("dw45-create");
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    add_line(&mut lines, "gamma.md");

    match diff(&listing, &lines) {
        Err(EditError::NameTaken { name, .. }) => assert_eq!(name, "gamma.md"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// A create colliding with a row the reader **removed** in the same edit.
/// Applied naively this is catastrophic: the create fails because the file is
/// still there, and then the delete succeeds — the reader loses the file and
/// gains nothing.
#[test]
fn test_dw_4_5_a_create_colliding_with_a_pending_delete_is_refused() {
    let dir = fixture("dw45-swap");
    let before = snapshot(&dir);
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    remove_line(&mut lines, "beta.md");
    add_line(&mut lines, "beta.md");

    match diff(&listing, &lines) {
        Err(EditError::NameTaken { name, .. }) => assert_eq!(name, "beta.md"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(snapshot(&dir), before);
    assert_eq!(
        fs::read_to_string(dir.join("beta.md")).expect("beta.md survived"),
        "beta-contents\n"
    );
}

/// The collider the diff cannot see: a name that did not exist when the
/// directory was read. The pre-flight refuses the **whole** plan, so the
/// rename that would have overwritten it never runs and neither does anything
/// else.
#[test]
fn test_dw_4_5_a_name_that_appeared_after_the_read_is_refused_before_anything_runs() {
    let dir = fixture("dw45-race");
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "alpha.md", "newcomer.md");
    remove_line(&mut lines, "gamma.md");
    let plan = diff(&listing, &lines).expect("legal at diff time");

    fs::write(dir.join("newcomer.md"), "ARRIVED-LATER\n").expect("write the collider");
    let before = snapshot(&dir);

    let results = plan.apply();
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(
        results.iter().all(|(_, result)| result.is_err()),
        "the whole plan must be refused: {results:?}"
    );
    assert_eq!(snapshot(&dir), before, "and nothing at all may have run");
    assert_eq!(
        fs::read_to_string(dir.join("newcomer.md")).expect("the collider survived"),
        "ARRIVED-LATER\n"
    );
    assert!(
        dir.join("gamma.md").exists(),
        "the delete must not have run either"
    );
    assert!(
        apply_summary(&results).starts_with("applied 0 of 2"),
        "{}",
        apply_summary(&results)
    );
}

/// A hidden file is not invisible to this: `read_dir` lists dotfiles, so the
/// listing holds `.hidden` and the diff refuses the collision itself.
#[test]
fn test_dw_4_5_a_dotfile_collision_is_refused() {
    let dir = fixture("dw45-dotfile");
    fs::write(dir.join(".hidden"), "hidden\n").expect("write dotfile");
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "alpha.md", ".hidden");

    match diff(&listing, &lines) {
        Err(EditError::NameTaken { name, .. }) => assert_eq!(name, ".hidden"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(
        fs::read_to_string(dir.join(".hidden")).expect("the dotfile survived"),
        "hidden\n"
    );
}

/// Whether the filesystem under `temp_dir` folds case — the fact the two
/// tests below branch on, measured rather than assumed.
fn filesystem_is_case_insensitive(dir: &Path) -> bool {
    let probe = dir.join("case-probe");
    fs::write(&probe, "probe").expect("write case probe");
    let folded = dir.join("CASE-PROBE").exists();
    fs::remove_file(&probe).expect("remove case probe");
    folded
}

/// `readme.md` → `README.md`. On a case-insensitive filesystem — macOS's
/// default, and this is macOS — the target "already exists" because it *is*
/// the source, and a pre-flight that only asked "is something there?" would
/// make the commonest rename in the world impossible.
#[test]
fn test_dw_4_5_a_case_only_rename_is_allowed() {
    let dir = scratch_dir("dw45-case");
    fs::write(dir.join("readme.md"), "readme-contents\n").expect("write readme");
    let folded = filesystem_is_case_insensitive(&dir);

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "readme.md", "README.md");
    let plan = diff(&listing, &lines).expect("a case change is a legal edit");

    // The whole reason the pre-flight needs an exception at all: where the
    // filesystem folds case, the rename's target "already exists" before a
    // single byte has moved, because it *is* the source.
    assert_eq!(
        dir.join("README.md").exists(),
        folded,
        "the target's prior existence is exactly the fold"
    );

    apply_expecting_success(&plan);

    let after = snapshot(&dir);
    assert_eq!(
        after.len(),
        1,
        "a case-only rename must not leave two files behind: {after:?}"
    );
    let (name, contents) = after.iter().next().expect("one entry");
    assert_eq!(name, OsString::from("README.md").as_os_str(), "{after:?}");
    assert_eq!(contents, "readme-contents\n");
}

/// The other half of the case rule, and the one that keeps it safe: on a
/// **case-sensitive** filesystem `ALPHA.md` is a different file, so the same
/// rename must be refused rather than allowed to overwrite it.
///
/// Skipped where the filesystem folds case, because there the two names
/// cannot name two files and the situation is unconstructible. That leaves
/// the inode half of `case_only_rename_of_the_same_file` unexercised on
/// macOS; it is exercised on any case-sensitive filesystem, and the case half
/// is exercised everywhere by the test above.
#[test]
fn test_a_case_only_rename_onto_a_genuinely_different_file_is_refused() {
    let dir = scratch_dir("case-distinct");
    fs::write(dir.join("alpha.md"), "lower\n").expect("write lower");
    if filesystem_is_case_insensitive(&dir) {
        return;
    }

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "alpha.md", "ALPHA.md");
    let plan = diff(&listing, &lines).expect("legal at diff time");

    fs::write(dir.join("ALPHA.md"), "UPPER\n").expect("write the distinct collider");
    let before = snapshot(&dir);
    let results = plan.apply();
    assert!(
        results.iter().all(|(_, result)| result.is_err()),
        "a distinct file must not be overwritten: {results:?}"
    );
    assert_eq!(snapshot(&dir), before);
}

// ------------------------------------------------------------------- DW-4.6

/// `a` → `b`, `b` → `a`. Refused, and — the assertion that matters — both
/// files still hold their own contents afterwards. Counting operations would
/// prove nothing: a plan of two renames applied in either order destroys one
/// of the two files.
#[test]
fn test_dw_4_6_a_rename_cycle_is_refused_and_neither_file_is_lost() {
    let dir = scratch_dir("dw46-cycle");
    fs::write(dir.join("a.md"), "AAA\n").expect("write a");
    fs::write(dir.join("b.md"), "BBB\n").expect("write b");
    let before = snapshot(&dir);

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_lines(&mut lines, &[("a.md", "b.md"), ("b.md", "a.md")]);

    let outcome = diff(&listing, &lines);
    assert!(
        matches!(outcome, Err(EditError::NameTaken { .. })),
        "expected a refusal, got {outcome:?}"
    );

    assert_eq!(snapshot(&dir), before);
    let survivors: Vec<String> = snapshot(&dir).into_values().collect();
    assert!(
        survivors.contains(&"AAA\n".to_string()) && survivors.contains(&"BBB\n".to_string()),
        "both files' contents must survive: {survivors:?}"
    );
}

/// A rename *chain* is refused by the same rule. Not a bug — a documented
/// trade: applying it needs an ordering, applying a cycle needs a temporary
/// name, and a partial failure mid-shuffle would strand the reader's data
/// under a name they never chose.
#[test]
fn test_a_rename_chain_is_refused_by_the_same_rule_that_refuses_a_cycle() {
    let dir = scratch_dir("chain");
    fs::write(dir.join("a.md"), "AAA\n").expect("write a");
    fs::write(dir.join("b.md"), "BBB\n").expect("write b");
    let before = snapshot(&dir);

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_lines(&mut lines, &[("a.md", "b.md"), ("b.md", "c.md")]);

    assert!(matches!(
        diff(&listing, &lines),
        Err(EditError::NameTaken { .. })
    ));
    assert_eq!(snapshot(&dir), before);
}

/// Two rows renamed to the same new name — a name nothing on disk holds, so
/// the occupancy rule does not catch it. Without the duplicate-name rule the
/// second rename would silently overwrite the first.
#[test]
fn test_dw_4_6_two_rows_renamed_to_the_same_name_are_refused() {
    let dir = fixture("dw46-dup");
    let before = snapshot(&dir);
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_lines(
        &mut lines,
        &[("alpha.md", "same.md"), ("beta.md", "same.md")],
    );

    match diff(&listing, &lines) {
        Err(EditError::DuplicateName { name, .. }) => assert_eq!(name, "same.md"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(snapshot(&dir), before);
}

/// A rename and a create landing on the same name, caught by the same rule.
#[test]
fn test_a_rename_and_a_create_of_the_same_name_are_refused() {
    let dir = fixture("dup-mixed");
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "alpha.md", "same.md");
    add_line(&mut lines, "same.md");

    assert!(matches!(
        diff(&listing, &lines),
        Err(EditError::DuplicateName { .. })
    ));
}

// ------------------------------------------------------------------- DW-4.7

/// A plan that fails partway says which operations landed and which did not,
/// and the summary never reads as success.
///
/// The failure is a non-empty directory, which is also DW-4.8's refusal: the
/// two deletes are independent, so the empty one goes and the full one does
/// not — and `sub/inner.md` is still there, which is what "no recursive
/// delete" means on disk.
#[test]
fn test_dw_4_7_a_delete_that_fails_is_reported_per_operation() {
    let dir = fixture("dw47-partial");
    fs::create_dir(dir.join("empty")).expect("create empty dir");

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    remove_line(&mut lines, "empty/");
    remove_line(&mut lines, "sub/");

    let plan = diff(&listing, &lines).expect("legal");
    let results = plan.apply();
    assert_eq!(results.len(), 2, "{results:?}");

    let landed: Vec<&Operation> = results
        .iter()
        .filter(|(_, result)| result.is_ok())
        .map(|(op, _)| op)
        .collect();
    let refused: Vec<&Operation> = results
        .iter()
        .filter(|(_, result)| result.is_err())
        .map(|(op, _)| op)
        .collect();
    assert_eq!(landed.len(), 1, "{results:?}");
    assert_eq!(refused.len(), 1, "{results:?}");
    assert!(landed[0].to_string().contains("empty"), "{landed:?}");
    assert!(refused[0].to_string().contains("sub"), "{refused:?}");

    assert!(!dir.join("empty").exists(), "the empty directory went");
    assert_eq!(
        fs::read_to_string(dir.join("sub").join("inner.md")).expect("inner survived"),
        "inner-contents\n",
        "a non-empty directory must survive whole"
    );

    let summary = apply_summary(&results);
    assert!(
        summary.starts_with("applied 1 of 2, 1 failed"),
        "the status row must not report success: {summary}"
    );
}

/// A failure in the rename phase stops **every** delete. The rename fails
/// because its source vanished between the diff and the write — the "an
/// edited row whose original entry was deleted meanwhile" case — and the file
/// the reader asked to delete is still there afterwards, because the
/// irreversible half of a plan does not run when the reversible half did not
/// land.
#[test]
fn test_dw_4_7_a_failure_in_the_rename_phase_stops_every_delete() {
    let dir = fixture("dw47-abort");
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "alpha.md", "renamed.md");
    remove_line(&mut lines, "beta.md");
    remove_line(&mut lines, "gamma.md");
    let plan = diff(&listing, &lines).expect("legal");

    fs::remove_file(dir.join("alpha.md")).expect("the source vanishes meanwhile");

    let results = plan.apply();
    assert_eq!(results.len(), 3, "{results:?}");
    assert!(
        results.iter().all(|(_, result)| result.is_err()),
        "{results:?}"
    );
    for (op, result) in results.iter().skip(1) {
        let message = result.as_ref().expect_err("a skipped delete").to_string();
        assert!(
            message.contains("not attempted"),
            "{op} should say it never ran: {message}"
        );
    }
    assert!(dir.join("beta.md").exists(), "no delete may have run");
    assert!(dir.join("gamma.md").exists(), "no delete may have run");

    let summary = apply_summary(&results);
    assert!(summary.starts_with("applied 0 of 3, 3 failed"), "{summary}");
}

/// The summary's only happy sentence, and its two unhappy ones, pinned —
/// because "applied 3 operations" appearing after a failure is the exact lie
/// DW-4.7 forbids.
#[test]
fn test_dw_4_7_the_status_summary_never_reports_success_when_anything_failed() {
    let ok = Operation::Create {
        path: PathBuf::from("/d/a"),
    };
    let bad = Operation::Delete {
        path: PathBuf::from("/d/b"),
        identity: None,
    };
    assert_eq!(apply_summary(&[]), "nothing to write");
    assert_eq!(
        apply_summary(&[(ok.clone(), Ok(()))]),
        "applied 1 operation"
    );
    assert_eq!(
        apply_summary(&[(ok.clone(), Ok(())), (bad.clone(), Ok(()))]),
        "applied 2 operations"
    );
    let mixed = apply_summary(&[
        (ok, Ok(())),
        (
            bad,
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "nope",
            )),
        ),
    ]);
    assert!(
        mixed.starts_with("applied 1 of 2, 1 failed — delete b: "),
        "{mixed}"
    );
    assert!(!mixed.starts_with("applied 2"), "{mixed}");
}

// ------------------------------------------------------------------- DW-4.8

/// An empty directory is removed; a non-empty one is refused, with everything
/// inside it intact. `remove_dir` is the enforcement — never `remove_dir_all`,
/// which is what makes the second half true.
#[test]
fn test_dw_4_8_an_empty_directory_is_removed_and_a_non_empty_one_is_refused() {
    let dir = fixture("dw48-dirs");
    fs::create_dir(dir.join("empty")).expect("create empty dir");
    let before = snapshot(&dir);

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    remove_line(&mut lines, "empty/");
    let plan = diff(&listing, &lines).expect("legal");
    apply_expecting_success(&plan);
    assert!(!dir.join("empty").exists());

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    remove_line(&mut lines, "sub/");
    let plan = diff(&listing, &lines).expect("legal");
    let results = plan.apply();
    assert!(
        results.iter().all(|(_, result)| result.is_err()),
        "a non-empty directory must be refused: {results:?}"
    );
    assert_eq!(
        before
            .get(OsString::from("sub").as_os_str())
            .expect("sub was in the snapshot"),
        snapshot(&dir)
            .get(OsString::from("sub").as_os_str())
            .expect("sub is still there"),
        "its contents must be untouched"
    );
}

/// A name containing a path separator is refused at diff time. This is the
/// traversal refusal too: `../escape.md` dies here, on the separator, before
/// anything asks what `..` means.
#[test]
fn test_dw_4_8_a_name_with_a_path_separator_is_refused_at_diff_time() {
    let dir = fixture("dw48-sep");
    let listing = Listing::read(&dir);

    for attempt in ["../escape.md", "sub/nested.md", "/absolute.md", "a/b"] {
        let mut lines = lines_of(&listing);
        rename_line(&mut lines, "alpha.md", attempt);
        match diff(&listing, &lines) {
            Err(EditError::IllegalName { fault, .. }) => {
                assert_eq!(fault, NameFault::Separator, "for `{attempt}`");
            }
            other => panic!("`{attempt}` must be refused, got {other:?}"),
        }

        let mut lines = lines_of(&listing);
        add_line(&mut lines, attempt);
        assert!(
            matches!(diff(&listing, &lines), Err(EditError::IllegalName { .. })),
            "creating `{attempt}` must be refused too"
        );
    }
}

/// `.` and `..` on their own are refused at diff time.
#[test]
fn test_dw_4_8_a_dot_or_dot_dot_name_is_refused_at_diff_time() {
    let dir = fixture("dw48-dot");
    let listing = Listing::read(&dir);
    for attempt in [".", ".."] {
        let mut lines = lines_of(&listing);
        rename_line(&mut lines, "alpha.md", attempt);
        match diff(&listing, &lines) {
            Err(EditError::IllegalName { fault, .. }) => {
                assert_eq!(fault, NameFault::Dot, "for `{attempt}`");
            }
            other => panic!("`{attempt}` must be refused, got {other:?}"),
        }
    }
}

/// A whitespace-only name is refused at diff time — it is legal on Unix and
/// indistinguishable from a blank row on screen.
#[test]
fn test_dw_4_8_a_whitespace_only_name_is_refused_at_diff_time() {
    let dir = fixture("dw48-blank");
    let listing = Listing::read(&dir);
    for attempt in [" ", "   ", "\t ", ""] {
        let mut lines = lines_of(&listing);
        rename_line(&mut lines, "alpha.md", attempt);
        match diff(&listing, &lines) {
            Err(EditError::IllegalName { fault, .. }) => {
                assert_eq!(fault, NameFault::Blank, "for {attempt:?}");
            }
            other => panic!("{attempt:?} must be refused, got {other:?}"),
        }
    }
}

/// A NUL or a line break in a name is refused before it can reach a syscall.
#[test]
fn test_a_nul_or_newline_in_a_name_is_refused_at_diff_time() {
    let dir = fixture("nul");
    let listing = Listing::read(&dir);
    for (attempt, expected) in [
        ("bad\0name.md", NameFault::Nul),
        ("bad\nname.md", NameFault::Newline),
        ("bad\rname.md", NameFault::Newline),
    ] {
        let mut lines = lines_of(&listing);
        rename_line(&mut lines, "alpha.md", attempt);
        match diff(&listing, &lines) {
            Err(EditError::IllegalName { fault, .. }) => {
                assert_eq!(fault, expected, "for {attempt:?}");
            }
            other => panic!("{attempt:?} must be refused, got {other:?}"),
        }
    }
}

/// A new row ending in `/` is refused rather than quietly creating a file.
#[test]
fn test_a_new_row_asking_for_a_directory_is_refused_rather_than_made_a_file() {
    let dir = fixture("dircreate");
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    add_line(&mut lines, "wanted/");

    match diff(&listing, &lines) {
        Err(EditError::DirectoryCreate { name, .. }) => assert_eq!(name, "wanted/"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(!dir.join("wanted").exists());
}

/// Deleting a symlink unlinks the link, never what it points at.
#[test]
fn test_deleting_a_symlink_row_never_follows_it() {
    let dir = scratch_dir("symlink-delete");
    let target_dir = scratch_dir("symlink-target");
    fs::write(target_dir.join("precious.md"), "PRECIOUS\n").expect("write target");
    std::os::unix::fs::symlink(target_dir.join("precious.md"), dir.join("link.md"))
        .expect("create symlink");

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    remove_line(&mut lines, "link.md");
    let plan = diff(&listing, &lines).expect("legal");
    apply_expecting_success(&plan);

    assert!(!dir.join("link.md").exists(), "the link went");
    assert_eq!(
        fs::read_to_string(target_dir.join("precious.md")).expect("the target survived"),
        "PRECIOUS\n"
    );
}

// ------------------------------------------------------------------- DW-4.9

/// A truncated listing produces **no** delete operations. The reader saw the
/// first 256 names `readdir` happened to yield, so absence from the buffer
/// cannot mean removal.
#[test]
fn test_dw_4_9_a_truncated_listing_produces_no_delete_operations() {
    let dir = fixture("dw49-truncated");
    let before = snapshot(&dir);

    // The same directory, listed twice: once honestly, once flagged as
    // capped. The only difference between the two plans must be the deletes.
    let honest = Listing::read(&dir);
    let capped = Listing::from_entries(honest.dir().to_path_buf(), honest.entries().to_vec(), true);

    let mut lines = lines_of(&honest);
    remove_line(&mut lines, "beta.md");
    remove_line(&mut lines, "gamma.md");

    let uncapped = diff(&honest, &lines).expect("legal");
    assert_eq!(uncapped.destructive().len(), 2, "the control case deletes");

    let plan = diff(&capped, &lines).expect("legal");
    assert!(
        plan.destructive().is_empty(),
        "a truncated listing must infer no deletes at all: {:?}",
        plan.operations()
    );
    assert!(plan.is_empty(), "{:?}", plan.operations());
    assert_eq!(plan.suppressed_deletes(), 2);
    assert!(
        plan.confirmation().contains("2 removed rows ignored"),
        "the reader must be told: {}",
        plan.confirmation()
    );

    apply_expecting_success(&plan);
    assert_eq!(snapshot(&dir), before, "nothing may have been deleted");
}

/// Truncation suppresses deletes and nothing else: a rename and a create in
/// the same edit still land, because both are named explicitly rather than
/// inferred from an absence.
#[test]
fn test_dw_4_9_a_truncated_listing_still_renames_and_creates() {
    let dir = fixture("dw49-mixed");
    let honest = Listing::read(&dir);
    let capped = Listing::from_entries(honest.dir().to_path_buf(), honest.entries().to_vec(), true);

    let mut lines = lines_of(&honest);
    rename_line(&mut lines, "alpha.md", "renamed.md");
    add_line(&mut lines, "fresh.md");
    remove_line(&mut lines, "beta.md");

    let plan = diff(&capped, &lines).expect("legal");
    assert_eq!(plan.destructive().len(), 0);
    assert_eq!(plan.len(), 2, "{:?}", plan.operations());
    assert_eq!(plan.suppressed_deletes(), 1);
    apply_expecting_success(&plan);

    let after = snapshot(&dir);
    assert!(after.contains_key(OsString::from("renamed.md").as_os_str()));
    assert!(after.contains_key(OsString::from("fresh.md").as_os_str()));
    assert!(
        after.contains_key(OsString::from("beta.md").as_os_str()),
        "the removed row must have been ignored, not applied"
    );
}

/// And the flag really does come from a real capped read, not only from the
/// test constructor: a directory over the 256-entry cap lists as truncated,
/// and an edit of it deletes nothing.
#[test]
fn test_dw_4_9_a_genuinely_capped_directory_deletes_nothing() {
    let dir = scratch_dir("dw49-real");
    for index in 0..300 {
        fs::write(dir.join(format!("file-{index:03}.md")), "x\n")
            .expect("write a cap-exceeding fixture");
    }
    let listing = Listing::read(&dir);
    assert!(listing.truncated(), "300 entries must exceed the cap");

    let before = snapshot(&dir);
    // Remove every row the reader could see.
    let plan = diff(&listing, &[]).expect("legal");
    assert!(
        plan.is_empty(),
        "clearing a capped buffer must ask for nothing: {:?}",
        plan.operations()
    );
    assert!(plan.suppressed_deletes() >= 256);
    apply_expecting_success(&plan);
    assert_eq!(snapshot(&dir), before, "all 300 files must still be there");
}

/// The control for the test above: clearing an **untruncated** buffer really
/// does delete everything, so the assertion there is about truncation and not
/// about an empty buffer being inert.
#[test]
fn test_clearing_an_untruncated_buffer_deletes_every_row() {
    let dir = scratch_dir("clear-all");
    fs::write(dir.join("one.md"), "1\n").expect("write");
    fs::write(dir.join("two.md"), "2\n").expect("write");
    let listing = Listing::read(&dir);
    assert!(!listing.truncated());

    let plan = diff(&listing, &[]).expect("legal");
    assert_eq!(plan.destructive().len(), 2, "{:?}", plan.operations());
    apply_expecting_success(&plan);
    assert!(snapshot(&dir).is_empty(), "{:?}", snapshot(&dir));
}

// ------------------------------------------------- non-UTF8 and malformed rows

/// A name that is not valid UTF-8 shows lossily, so leaving its row alone is
/// a no-op, editing it is refused — a rename would be a guess about bytes the
/// reader never saw — and removing it still works, because a delete acts on
/// the captured path and never on the text.
///
/// Driven through `Listing::from_entries` rather than a real file, because
/// APFS refuses to create such a name at all (`EILSEQ`, measured — the first
/// draft of this test failed on exactly that). The disk half is
/// `test_a_non_utf8_row_round_trips_on_a_filesystem_that_allows_one` below,
/// which runs wherever the filesystem permits it and returns early where it
/// does not.
#[test]
fn test_a_non_utf8_row_is_untouchable_by_rename_but_not_by_removal() {
    let dir = PathBuf::from("/listed");
    let raw = OsString::from_vec(b"caf\xe9.md".to_vec());
    let listing = Listing::from_entries(
        dir.clone(),
        vec![Entry {
            path: dir.join(&raw),
            name: raw.clone(),
            kind: EntryKind::Document,
        }],
        false,
    );

    let lines = lines_of(&listing);
    assert_eq!(lines.len(), 1, "the lossy row is still offered: {lines:?}");
    assert!(
        EditPlan::diff(&listing, &lines).expect("legal").is_empty(),
        "left alone, it asks for nothing"
    );

    let edited = vec!["0\tcafe.md".to_string()];
    assert!(
        matches!(
            EditPlan::diff(&listing, &edited),
            Err(EditError::NonUtf8Rename { .. })
        ),
        "renaming a lossily-shown name must be refused"
    );

    let plan = EditPlan::diff(&listing, &[]).expect("legal");
    // Matched on the path rather than compared against a whole `Operation`:
    // a real delete now also carries the identity the read recorded, which a
    // test cannot spell and should not need to.
    match plan.destructive() {
        [Operation::Delete { path, .. }] => assert_eq!(
            path,
            &dir.join(&raw),
            "the delete names the captured path, byte for byte"
        ),
        other => panic!("expected exactly one delete, got {other:?}"),
    }
}

/// The same behaviour end to end on disk, where the filesystem allows a
/// non-UTF8 name. APFS does not, so on macOS this returns early and the
/// coverage above is what stands; on any Linux filesystem it runs.
#[test]
fn test_a_non_utf8_row_round_trips_on_a_filesystem_that_allows_one() {
    let dir = scratch_dir("non-utf8-disk");
    let raw = OsString::from_vec(b"caf\xe9.md".to_vec());
    if fs::write(dir.join(&raw), "cafe\n").is_err() {
        return;
    }
    let before = snapshot(&dir);

    let listing = Listing::read(&dir);
    let plan = diff(&listing, &lines_of(&listing)).expect("legal");
    apply_expecting_success(&plan);
    assert_eq!(snapshot(&dir), before, "left alone means untouched");

    let plan = diff(&listing, &[]).expect("legal");
    apply_expecting_success(&plan);
    assert!(snapshot(&dir).is_empty(), "{:?}", snapshot(&dir));
}

/// A parent row that points back *into* the directory it heads is not a
/// listing this module will edit: its recorded path would alias a name an
/// edit can legitimately produce. Found by the randomized suite, not by
/// inspection.
#[test]
fn test_a_parent_row_pointing_into_its_own_directory_is_refused() {
    let dir = PathBuf::from("/listed");
    let listing = Listing::from_entries(
        dir.clone(),
        vec![Entry {
            name: OsString::from(".."),
            path: dir.join("inside"),
            kind: EntryKind::Parent,
        }],
        false,
    );
    assert!(matches!(
        EditPlan::diff(&listing, &[]),
        Err(EditError::InconsistentListing { .. })
    ));

    // The honest shape is accepted.
    let listing = Listing::from_entries(
        dir.clone(),
        vec![Entry {
            name: OsString::from(".."),
            path: PathBuf::from("/"),
            kind: EntryKind::Parent,
        }],
        false,
    );
    assert!(EditPlan::diff(&listing, &[]).expect("legal").is_empty());
}

/// The parent row is never emitted into the buffer, and a line that names it
/// anyway is refused rather than turned into an operation on the directory
/// above.
#[test]
fn test_a_line_naming_the_parent_row_is_refused() {
    let dir = fixture("parent-row");
    let listing = Listing::read(&dir);
    assert_eq!(listing.entries()[0].kind, EntryKind::Parent);
    assert!(
        !lines_of(&listing)
            .iter()
            .any(|line| line.starts_with("0\t")),
        "the parent row must never be emitted"
    );

    assert!(matches!(
        diff(&listing, &["0\tsomething".to_string()]),
        Err(EditError::ParentRow { .. })
    ));
    // And its absence is not a delete either.
    let plan = diff(&listing, &lines_of(&listing)).expect("legal");
    assert!(plan.is_empty(), "{:?}", plan.operations());
}

/// Rows a reader cannot produce but a fuzzer can, each refused by name.
#[test]
fn test_malformed_rows_are_refused_rather_than_guessed_at() {
    let dir = fixture("malformed");
    let listing = Listing::read(&dir);
    let count = listing.entries().len();

    assert!(matches!(
        diff(&listing, &["no tab here".to_string()]),
        Err(EditError::MalformedRow { line: 0 })
    ));
    assert!(matches!(
        diff(&listing, &["notanumber\tx.md".to_string()]),
        Err(EditError::MalformedRow { line: 0 })
    ));
    assert!(matches!(
        diff(&listing, &[format!("{count}\tx.md")]),
        Err(EditError::UnknownRow { line: 0, .. })
    ));
    assert!(matches!(
        diff(
            &listing,
            &["1\talpha.md".to_string(), "1\tx.md".to_string()]
        ),
        Err(EditError::DuplicateRow { .. })
    ));
}

/// A listing whose entries do not agree with their own directory is refused
/// outright. No real `read_dir` produces one; a synthetic listing can, and
/// without this an entry named `..` would become a delete of the parent.
#[test]
fn test_a_listing_whose_entries_escape_its_directory_is_refused() {
    let dir = PathBuf::from("/listed");
    for entry in [
        Entry {
            name: OsString::from(".."),
            path: dir.join(".."),
            kind: EntryKind::Directory,
        },
        Entry {
            name: OsString::from("sneaky"),
            path: PathBuf::from("/elsewhere/sneaky"),
            kind: EntryKind::Document,
        },
        Entry {
            name: OsString::from("a/b"),
            path: dir.join("a/b"),
            kind: EntryKind::Document,
        },
        Entry {
            name: OsString::from("."),
            path: dir.join("."),
            kind: EntryKind::Directory,
        },
        Entry {
            name: OsString::new(),
            path: dir.clone(),
            kind: EntryKind::Document,
        },
        Entry {
            name: OsString::from_vec(b"nul\0name".to_vec()),
            path: dir.join(OsString::from_vec(b"nul\0name".to_vec())),
            kind: EntryKind::Document,
        },
    ] {
        let listing = Listing::from_entries(dir.clone(), vec![entry.clone()], false);
        assert!(
            matches!(
                EditPlan::diff(&listing, &[]),
                Err(EditError::InconsistentListing { .. })
            ),
            "{entry:?} must be refused"
        );
    }

    // Two entries under one name is not a directory either: `readdir` yields
    // each name once, and an ambiguous listing must not be diffed.
    let twice = Entry {
        name: OsString::from("shared"),
        path: dir.join("shared"),
        kind: EntryKind::Document,
    };
    let listing = Listing::from_entries(dir.clone(), vec![twice.clone(), twice], false);
    assert!(matches!(
        EditPlan::diff(&listing, &[]),
        Err(EditError::InconsistentListing { .. })
    ));

    // And the honest shape of the same listing is accepted and diffed.
    let listing = Listing::from_entries(
        dir.clone(),
        vec![Entry {
            name: OsString::from("shared"),
            path: dir.join("shared"),
            kind: EntryKind::Document,
        }],
        false,
    );
    assert_eq!(
        EditPlan::diff(&listing, &[])
            .expect("legal")
            .destructive()
            .len(),
        1
    );
}

// ------------------------------------------------------------- the buffer view

/// The buffer paints what the listing painted, plus a two-column gutter that
/// says what each row is about to become.
#[test]
fn test_the_buffer_marks_edited_new_and_removed_rows() {
    let dir = fixture("markers");
    fs::write(dir.join("delta.md"), "delta\n").expect("write delta");
    fs::write(dir.join("epsilon.md"), "epsilon\n").expect("write epsilon");
    let listing = Listing::read(&dir);
    let mut buffer = EditBuffer::new(&listing);
    assert!(!buffer.is_dirty());
    assert!(!buffer.is_empty());
    assert_eq!(buffer.len(), listing.entries().len());
    assert_eq!(
        buffer.indicator(),
        "[ ] editing — w writes, Esc discards",
        "a clean buffer says so"
    );

    // Rows: 0 `../`, 1 alpha.md, 2 beta.md, 3 delta.md, 4 epsilon.md,
    // 5 gamma.md, 6 sub/. Two renamed, one new, three removed.
    buffer.push_char(1, 'X');
    buffer.push_char(5, 'Y');
    buffer.toggle_removed(2);
    buffer.toggle_removed(3);
    buffer.toggle_removed(6);
    let added = buffer.insert_new(3);
    for ch in "fresh.md".chars() {
        buffer.push_char(added, ch);
    }

    // The dirty indicator's three counts, which nothing used to assert. A
    // mutation sweep found `tally`'s `!=` could become `==` — counting every
    // *unchanged* row as renamed — with the whole suite still green. It is
    // user-facing truth about what is about to happen.
    assert_eq!(
        buffer.indicator(),
        "[+] 2 renamed, 1 new, 3 removed — w writes, Esc discards",
        "the indicator must count what actually changed"
    );

    let rows = buffer.rows(40, 0);
    let texts: Vec<&str> = rows.iter().map(|row| row.text.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "  ../",
            "~ alpha.mdX",
            "- beta.md",
            "- delta.md",
            "+ fresh.md",
            "  epsilon.md",
            "~ gamma.mdY",
            "- sub/",
        ]
    );
    assert_eq!(rows[2].style, RowStyle::Dimmed, "a doomed row dims");
    assert_eq!(rows[0].style, RowStyle::Selected, "the cursor is on row 0");
    assert!(buffer.is_dirty());
    assert_eq!(
        buffer.indicator(),
        "[+] 2 renamed, 1 new, 3 removed — w writes, Esc discards"
    );

    // And the lines it hands the diff: the parent row and every removed row
    // are absent, the new row carries no id, and the ids are the *listing's*
    // indices rather than the buffer's shifted ones.
    assert_eq!(
        buffer.lines(),
        vec![
            "1\talpha.mdX".to_string(),
            "\tfresh.md".to_string(),
            "4\tepsilon.md".to_string(),
            "5\tgamma.mdY".to_string(),
        ]
    );

    // Removal wins over newness: a row the reader added and then removed is
    // gone from the plan, not pending in it.
    buffer.toggle_removed(4);
    assert_eq!(buffer.rows(40, 0)[4].text, "- fresh.md");
    assert!(
        !buffer.lines().iter().any(|line| line.contains("fresh.md")),
        "{:?}",
        buffer.lines()
    );
}

/// The buffer windows exactly as the listing does: a buffer taller than the
/// viewport still shows the cursor on the right row, and never paints more
/// rows than it was given.
#[test]
fn test_a_buffer_taller_than_the_viewport_still_shows_the_cursor() {
    let dir = scratch_dir("windowing");
    for index in 0..30 {
        fs::write(dir.join(format!("f-{index:02}.md")), "x\n").expect("write");
    }
    let buffer = EditBuffer::new(&Listing::read(&dir));
    assert_eq!(buffer.len(), 31, "thirty files and a parent row");

    for selected in [0usize, 1, 15, 30, 99] {
        let rows = buffer.rows(7, selected);
        assert_eq!(rows.len(), 7, "selected {selected}: {rows:?}");
        let cursors: Vec<&str> = rows
            .iter()
            .filter(|row| row.style == RowStyle::Selected)
            .map(|row| row.text.as_str())
            .collect();
        assert_eq!(cursors.len(), 1, "selected {selected}: {rows:?}");
        let clamped = selected.min(buffer.len() - 1);
        let expected = if clamped == 0 {
            "  ../".to_string()
        } else {
            format!("  f-{:02}.md", clamped - 1)
        };
        assert_eq!(cursors[0], expected, "selected {selected}");
    }
    assert!(
        buffer.rows(0, 0).is_empty(),
        "a zero-row viewport paints nothing"
    );
}

/// `Esc` on a row puts it back exactly as it was, and the parent row cannot
/// be typed into or removed at all.
#[test]
fn test_a_row_reverts_and_the_parent_row_refuses_every_edit() {
    let dir = fixture("revert");
    let listing = Listing::read(&dir);
    let mut buffer = EditBuffer::new(&listing);

    buffer.push_char(1, 'X');
    assert_eq!(buffer.text(1), Some("alpha.mdX"));
    buffer.revert(1);
    assert_eq!(buffer.text(1), Some("alpha.md"));
    assert!(!buffer.is_dirty());

    assert!(!buffer.is_editable(0));
    buffer.push_char(0, 'X');
    buffer.toggle_removed(0);
    assert_eq!(buffer.text(0), Some("../"));
    assert!(
        !buffer.is_dirty(),
        "the parent row cannot make a buffer dirty"
    );
}

/// A capped listing says so in the buffer itself, not only in the
/// confirmation — the reader needs to know before they start removing rows.
#[test]
fn test_a_capped_buffer_carries_a_notice_row() {
    let dir = fixture("capped-notice");
    let honest = Listing::read(&dir);
    let capped = Listing::from_entries(honest.dir().to_path_buf(), honest.entries().to_vec(), true);
    let rows = EditBuffer::new(&capped).rows(40, 0);
    assert!(
        rows[0].text.contains("removed rows will be ignored"),
        "{:?}",
        rows[0]
    );
}

/// An empty new row is not a request to create a file called nothing.
#[test]
fn test_an_empty_new_row_asks_for_nothing() {
    let dir = fixture("empty-new");
    let listing = Listing::read(&dir);
    let mut buffer = EditBuffer::new(&listing);
    buffer.insert_new(1);
    assert!(buffer.is_dirty(), "the reader did press a key");
    let plan = EditPlan::diff(&listing, &buffer.lines()).expect("legal");
    assert!(plan.is_empty(), "{:?}", plan.operations());
}

/// Unsaved edits refuse to be navigated away from, and `Esc` discards them
/// without touching the disk.
#[test]
fn test_unsaved_edits_block_navigation_and_esc_discards_them() {
    let dir = fixture("unsaved");
    let before = snapshot(&dir);
    let mut state = app_state();
    state.install_listing(Listing::read(&dir), 1, true);

    press_char(&mut state, 'd');
    press(&mut state, KeyCode::Enter);
    assert!(
        state.take_action().is_none(),
        "Enter must not descend with unsaved edits"
    );
    assert!(status_text(&mut state).contains("unsaved edits"));

    press_char(&mut state, '-');
    assert!(state.take_action().is_none(), "`-` must not ascend either");

    press(&mut state, KeyCode::Esc);
    assert!(status_text(&mut state).contains("edits discarded"));
    assert_eq!(snapshot(&dir), before);
    // And the explorer is back to its read-only self: `Enter` descends again.
    press_char(&mut state, 'j');
    press_char(&mut state, 'j');
    press_char(&mut state, 'j');
    press(&mut state, KeyCode::Enter);
    assert!(
        matches!(state.take_action(), Some(PendingAction::ListDirectory(_))),
        "discarding must restore the read-only key table"
    );
}

/// `w` over a buffer that asks for nothing says so rather than putting an
/// empty question on screen.
#[test]
fn test_writing_an_unchanged_buffer_says_there_is_nothing_to_write() {
    let dir = fixture("nothing");
    let mut state = app_state();
    state.install_listing(Listing::read(&dir), 1, true);
    press_char(&mut state, 'w');
    assert!(status_text(&mut state).contains("no changes to write"));
    assert!(state.take_action().is_none());
}

/// A refusal reaches the reader on the status row, naming the row that caused
/// it, instead of a plan that would have destroyed something.
#[test]
fn test_a_refused_edit_puts_the_reason_on_the_status_row() {
    let dir = fixture("refusal-status");
    let mut state = app_state();
    state.install_listing(Listing::read(&dir), 1, true);

    press_char(&mut state, 'i');
    retype(&mut state, "beta.md");
    press_char(&mut state, 'w');

    let message = status_text(&mut state);
    assert!(
        message.contains("`beta.md` already exists here"),
        "{message}"
    );
    assert!(state.take_action().is_none());
}

// ------------------------------------------------------------------ DW-4.10

/// xorshift64: the same generator `explore_listing.rs` uses, so both
/// randomized suites in this crate report a seed the same way.
struct Xorshift64(u64);

impl Xorshift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_usize(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next() % bound as u64) as usize
        }
    }
}

/// `Listing::rows`' private `row_text`, mirrored — the same three lines
/// `explore_listing.rs` mirrors, and for the same reason: without it "the
/// reader left this row alone" cannot be computed independently of the code
/// under test.
fn expected_row_text(entry: &Entry) -> String {
    match entry.kind {
        EntryKind::Parent => "../".to_string(),
        EntryKind::Directory => format!("{}/", entry.name.to_string_lossy()),
        EntryKind::Document | EntryKind::Unopenable => entry.name.to_string_lossy().into_owned(),
    }
}

/// An arbitrary entry name.
///
/// **Weighted, and the weighting is the difference between a test and a
/// formality.** The names that no `read_dir` could produce — `..`, `.`, the
/// empty string, one with a separator — must appear often enough to prove the
/// listing barricade refuses them, and rarely enough that most listings still
/// reach the diff. At the first cut they were 60% of all names, every listing
/// of five was refused before a single line was read, and only 6% of 20 000
/// iterations produced a plan at all; the acceptance floor at the bottom of
/// the test is what caught that.
fn arbitrary_name(rng: &mut Xorshift64, index: usize) -> OsString {
    match rng.next() % 64 {
        0 => OsString::from(".."),
        1 => OsString::from("."),
        2 => OsString::new(),
        3 => OsString::from(format!("with/slash-{index}")),
        // Deliberately not index-prefixed: two of these in one listing is the
        // duplicate-name case, which must be refused rather than diffed.
        4 | 5 => OsString::from("shared"),
        6..=11 => OsString::from_vec({
            let mut bytes = format!("raw-{index}-").into_bytes();
            bytes.extend((0..rng.next_usize(6)).map(|_| (rng.next() % 256) as u8));
            bytes
        }),
        _ => OsString::from(format!("name-{index}")),
    }
}

/// An arbitrary line body: names, near-names, and things that are not names.
fn arbitrary_text(rng: &mut Xorshift64) -> String {
    match rng.next() % 12 {
        0 => String::new(),
        1 => "   ".to_string(),
        2 => "..".to_string(),
        3 => ".".to_string(),
        4 => "../escape".to_string(),
        5 => "sub/nested".to_string(),
        6 => "shared".to_string(),
        7 => "with\ttab".to_string(),
        8 => "with\nbreak".to_string(),
        9 => "trailing/".to_string(),
        10 => "\u{0}nul".to_string(),
        _ => format!("gen-{}", rng.next() % 32),
    }
}

/// One arbitrary (listing, edited buffer) pair. The buffer starts as the
/// honest projection of the listing and is then mutated, which is what keeps
/// most iterations in the interesting region — a purely random `Vec<String>`
/// would be refused as malformed almost every time and prove very little.
fn arbitrary_case(rng: &mut Xorshift64) -> (Listing, Vec<String>) {
    let dir = PathBuf::from("/listed/dir");
    let count = rng.next_usize(10);
    let entries: Vec<Entry> = (0..count)
        .map(|index| {
            let kind = match rng.next() % 5 {
                0 => EntryKind::Parent,
                1 => EntryKind::Directory,
                2 | 3 => EntryKind::Document,
                _ => EntryKind::Unopenable,
            };
            let name = arbitrary_name(rng, index);
            // Usually the path a real read would have recorded; occasionally
            // one that escapes, to prove the barricade refuses rather than
            // emits. A parent row's honest path is the directory *above*, so
            // it gets the opposite default — the two are not the same
            // distribution, and giving `../` a child path by default refused
            // almost every listing that had one.
            let path = match (kind, rng.next().is_multiple_of(24)) {
                (EntryKind::Parent, false) => PathBuf::from("/listed"),
                (EntryKind::Parent, true) => dir.join(&name),
                (EntryKind::Directory | EntryKind::Document | EntryKind::Unopenable, false) => {
                    dir.join(&name)
                }
                (EntryKind::Directory | EntryKind::Document | EntryKind::Unopenable, true) => {
                    PathBuf::from("/elsewhere").join(&name)
                }
            };
            Entry { name, path, kind }
        })
        .collect();

    let listing = Listing::from_parts(
        dir,
        entries,
        rng.next().is_multiple_of(4),
        None,
        rng.next_usize(3),
    );
    let mut lines = EditBuffer::new(&listing).lines();
    for _ in 0..rng.next_usize(7) {
        let pick = rng.next() % 6;
        if lines.is_empty() || pick == 1 {
            lines.push(match rng.next() % 3 {
                0 => format!("\t{}", arbitrary_text(rng)),
                1 => arbitrary_text(rng),
                _ => format!("{}\t{}", rng.next_usize(12), arbitrary_text(rng)),
            });
            continue;
        }
        let at = rng.next_usize(lines.len());
        match pick {
            0 => {
                lines.remove(at);
            }
            2 => {
                let id = lines[at]
                    .split_once('\t')
                    .map(|(id, _)| id.to_string())
                    .unwrap_or_default();
                lines[at] = format!("{id}\t{}", arbitrary_text(rng));
            }
            3 => {
                let other = rng.next_usize(lines.len());
                lines.swap(at, other);
            }
            4 => {
                let copy = lines[at].clone();
                lines.push(copy);
            }
            _ => lines.reverse(),
        }
    }
    (listing, lines)
}

/// DW-4.10(b): over 20 000 arbitrary listings and arbitrary edited buffers,
/// `EditPlan::diff` never panics and never produces a plan that violates
/// either safety invariant — **no operation targets a path outside the listed
/// directory**, and **no entry the reader left untouched appears in any
/// operation**.
///
/// The stable-toolchain twin of `crates/stele/fuzz/fuzz_targets/edit_plan.rs`,
/// which asserts the same two things over libFuzzer-chosen bytes. Neither
/// suite ever calls `apply`: this is the *diff*'s contract, and a randomized
/// test that wrote to a disk would be a randomized test that deleted things.
#[test]
fn test_dw_4_10_arbitrary_edits_never_panic_and_never_touch_an_untouched_entry() {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15)
        | 1; // xorshift64 cannot start at zero

    let mut rng = Xorshift64(seed);
    let mut plans = 0u32;
    for iteration in 0..20_000u32 {
        let (listing, lines) = arbitrary_case(&mut rng);
        let context = || format!("seed {seed:#x}, iteration {iteration}");

        // Reaching the next line at all is the no-panic half.
        let Ok(plan) = EditPlan::diff(&listing, &lines) else {
            continue;
        };
        plans += 1;
        let dir = listing.dir();

        // Invariant one: nothing leaves the listed directory.
        for op in plan.operations() {
            for path in paths_of(op) {
                assert_eq!(
                    path.parent(),
                    Some(dir),
                    "an operation left the listed directory ({}): {op:?}",
                    context()
                );
                assert!(
                    path.file_name().is_some(),
                    "an operation has no final component ({}): {op:?}",
                    context()
                );
            }
        }

        // Invariant two: an entry whose row survived unchanged is in no
        // operation whatsoever.
        for (index, entry) in listing.entries().iter().enumerate() {
            let untouched = lines
                .iter()
                .any(|line| *line == format!("{index}\t{}", expected_row_text(entry)));
            if !untouched && entry.kind != EntryKind::Parent {
                continue;
            }
            for op in plan.operations() {
                for path in paths_of(op) {
                    assert_ne!(
                        path,
                        entry.path,
                        "an untouched entry appeared in an operation ({}): {op:?}",
                        context()
                    );
                }
            }
        }

        // The ordering the destructive slice is defined in terms of.
        let ops = plan.operations();
        let first_delete = ops.len() - plan.destructive().len();
        assert!(
            ops[..first_delete]
                .iter()
                .all(|op| !matches!(op, Operation::Delete { .. })),
            "a delete was ordered before the creates ({})",
            context()
        );
        assert!(
            plan.destructive()
                .iter()
                .all(|op| matches!(op, Operation::Delete { .. })),
            "destructive() must be exactly the deletes ({})",
            context()
        );
        assert!(
            ops[..first_delete]
                .iter()
                .position(|op| matches!(op, Operation::Create { .. }))
                .is_none_or(|create| !ops[create..first_delete]
                    .iter()
                    .any(|op| matches!(op, Operation::Rename { .. }))),
            "renames must all precede creates ({})",
            context()
        );

        // The truncation guard.
        if listing.truncated() {
            assert!(
                plan.destructive().is_empty(),
                "a truncated listing must produce no deletes ({}): {:?}",
                context(),
                plan.operations()
            );
        } else {
            assert_eq!(
                plan.suppressed_deletes(),
                0,
                "only truncation suppresses deletes ({})",
                context()
            );
        }
    }
    assert!(
        plans > 3_000,
        "the generator must reach accepted plans often enough to prove anything: \
         only {plans} of 20 000 (seed {seed:#x})"
    );
}

/// Every path an operation names.
fn paths_of(op: &Operation) -> Vec<&Path> {
    match op {
        Operation::Rename { from, to } => vec![from.as_path(), to.as_path()],
        Operation::Create { path } => vec![path.as_path()],
        Operation::Delete { path, .. } => vec![path.as_path()],
    }
}

// ------------------------------------------------- the real binary, end to end

/// The whole feature through **the shipped binary**, on a real pty.
///
/// Everything above this line stops at `AppState` and `EditPlan`. The one
/// thing neither can reach is `main.rs`'s `PendingAction::ApplyEdits` arm —
/// the only place in the program that calls `apply`, and therefore the only
/// place a correct plan can still fail to become a correct directory. Phase
/// 3 learned this the hard way twice: a DW-tagged test named for the
/// explorer that only checked terminal restore stayed green with the
/// explorer disabled outright. So this drives the keys a reader would press
/// and then looks at the disk.
mod real_binary {
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    use crate::common::pty::{
        ChildStdin, Pty, RESTORE, contains, drain_to_quiet, read_one_frame, read_until,
        spawn_viewer,
    };
    use crate::common::render::render_row;

    const COLS: usize = 80;
    const STATUS_ROW: u16 = 24;
    const CONTENT_ROWS: u16 = 23;
    const DEADLINE: Duration = Duration::from_secs(5);

    /// A directory whose listing is exactly `../`, `alpha.md`, `beta.md` — so
    /// the rooted explorer lands on `alpha.md` and one `j` reaches `beta.md`.
    fn fixture(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stele-edit-pty-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create the fixture");
        fs::write(dir.join("alpha.md"), "alpha-marker\n").expect("write alpha");
        fs::write(dir.join("beta.md"), "beta-marker\n").expect("write beta");
        dir
    }

    fn screen(frame: &str) -> String {
        (1..=CONTENT_ROWS)
            .map(|row| render_row(frame, row, COLS))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn status(frame: &str) -> String {
        render_row(frame, STATUS_ROW, COLS).trim_end().to_string()
    }

    /// Types `keys`, waits for the wire to go quiet, then provokes one clean
    /// frame with a key that every one of the explorer's three key tables
    /// deliberately ignores.
    ///
    /// `x` is that key — but **only outside row editing**, where it would be
    /// a character of the filename. Every call below is made from a state
    /// where the reader is not typing into a row.
    fn settle(pty: &Pty, keys: &[u8]) -> String {
        drain_to_quiet(pty.master_fd());
        pty.type_bytes(keys);
        drain_to_quiet(pty.master_fd());
        pty.type_bytes(b"x");
        read_one_frame(pty.master_fd(), DEADLINE)
    }

    /// A rename, a create and a delete typed into the overlay and written —
    /// then the directory itself, which is the only thing that settles it.
    #[test]
    fn test_dw_4_1_dw_4_2_dw_4_7_an_edit_typed_into_the_real_binary_reaches_the_disk() {
        let dir = fixture("apply");
        let pty = Pty::open();
        let mut child = spawn_viewer(&pty, &[dir.as_os_str()], ChildStdin::Tty);
        let master = pty.master_fd();
        read_until(master, b"alpha.md", DEADLINE);

        // `i`, clear the row, retype it, commit. 0x7f is what a terminal
        // sends for Backspace; the extra presses past the start are no-ops.
        let mut edit = vec![b'i'];
        edit.extend(std::iter::repeat_n(0x7f, 16));
        edit.extend_from_slice(b"renamed.md\r");
        // Then `j` onto beta.md and `d` to mark it, and `o` for a new row.
        edit.extend_from_slice(b"jdo");
        edit.extend_from_slice(b"created.md\r");
        let buffer = settle(&pty, &edit);

        let body = screen(&buffer);
        assert!(body.contains("~ renamed.md"), "{body}");
        assert!(body.contains("- beta.md"), "{body}");
        assert!(body.contains("+ created.md"), "{body}");
        assert!(
            status(&buffer).starts_with("[+] 1 renamed, 1 new, 1 removed — w writes"),
            "the whole dirty indicator must survive an ordinary directory path: {}",
            status(&buffer)
        );

        // Nothing has happened on disk yet.
        assert!(dir.join("alpha.md").exists(), "no write before the gate");
        assert!(dir.join("beta.md").exists(), "no write before the gate");

        let gate = settle(&pty, b"w");
        let question = status(&gate);
        assert!(
            question.contains("DELETE 1 (beta.md)") && question.contains("[y/n]"),
            "the gate must name the doomed file: {question}"
        );
        assert!(
            dir.join("beta.md").exists(),
            "the question alone must change nothing"
        );

        drain_to_quiet(master);
        pty.type_bytes(b"y");
        let applied = read_until(master, b"applied 3 operations", DEADLINE);
        assert!(
            contains(&applied, b"applied 3 operations"),
            "the status row must report the write: {}",
            String::from_utf8_lossy(&applied)
        );

        assert_eq!(
            fs::read_to_string(dir.join("renamed.md")).expect("the rename landed"),
            "alpha-marker\n"
        );
        assert_eq!(
            fs::read_to_string(dir.join("created.md")).expect("the create landed"),
            ""
        );
        assert!(!dir.join("alpha.md").exists(), "the old name is gone");
        assert!(!dir.join("beta.md").exists(), "the delete landed");

        // And the explorer re-listed itself onto the new names.
        let after = screen(&settle(&pty, b""));
        assert!(after.contains("renamed.md"), "{after}");
        assert!(after.contains("created.md"), "{after}");
        assert!(!after.contains("beta.md"), "{after}");

        drain_to_quiet(master);
        pty.type_bytes(b"q");
        let end = read_until(master, RESTORE, Duration::from_secs(10));
        assert!(contains(&end, RESTORE), "`q` must restore the terminal");
        assert_eq!(child.wait().expect("wait").code(), Some(0));
    }

    /// The other half of DW-4.7 through the real binary: a plan that fails
    /// partway must put the failure on the status row, not a success.
    #[test]
    fn test_dw_4_7_a_partial_failure_reaches_the_real_status_row() {
        let dir = fixture("partial");
        fs::create_dir(dir.join("full")).expect("create a non-empty directory");
        fs::write(dir.join("full").join("keep.md"), "KEEP\n").expect("write inside it");

        let pty = Pty::open();
        let mut child = spawn_viewer(&pty, &[dir.as_os_str()], ChildStdin::Tty);
        let master = pty.master_fd();
        read_until(master, b"alpha.md", DEADLINE);

        // Rows: `../`, alpha.md, beta.md, full/. Mark alpha.md and full/.
        let marked = settle(&pty, b"djjd");
        assert!(screen(&marked).contains("- full/"), "{}", screen(&marked));

        let gate = settle(&pty, b"w");
        assert!(status(&gate).contains("DELETE 2"), "{}", status(&gate));

        drain_to_quiet(master);
        pty.type_bytes(b"y");
        let applied = read_until(master, b"applied 1 of 2", DEADLINE);
        let text = String::from_utf8_lossy(&applied);
        assert!(
            contains(&applied, b"applied 1 of 2"),
            "the status row must not report success: {text}"
        );

        assert!(!dir.join("alpha.md").exists(), "the empty target went");
        assert_eq!(
            fs::read_to_string(dir.join("full").join("keep.md"))
                .expect("a non-empty directory survives whole"),
            "KEEP\n"
        );

        drain_to_quiet(master);
        pty.type_bytes(b"q");
        read_until(master, RESTORE, Duration::from_secs(10));
        let _ = child.wait();
    }
}

// ------------------------------------------------ the buffer/listing coupling

/// Installing a listing drops any buffer over the old one.
///
/// **The structural guarantee behind the whole id scheme.** A buffer's lines
/// carry indices into the listing they were built from; a buffer that
/// survived a re-list would name different files than the reader typed over,
/// and the diff would rename or delete them without hesitating. Making the
/// drop part of `install_listing` — the one seam every listing arrives
/// through — is what turns "do not do that" into "cannot".
#[test]
fn test_installing_a_listing_drops_the_buffer_built_over_the_previous_one() {
    let first = fixture("coupling-first");
    let second = scratch_dir("coupling-second");
    fs::write(second.join("other.md"), "other\n").expect("write");

    let mut state = app_state();
    state.install_listing(Listing::read(&first), 1, true);
    press_char(&mut state, 'd'); // a dirty buffer over the first listing
    assert!(
        state
            .explore_rows(10)
            .iter()
            .any(|row| row.text.starts_with("- ")),
        "the buffer is up: {:?}",
        state.explore_rows(10)
    );

    state.install_listing(Listing::read(&second), 1, true);
    let rows = state.explore_rows(10);
    assert!(
        rows.iter().all(|row| !row.text.starts_with("- ")),
        "the old buffer must be gone: {rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.text == "other.md"),
        "and the new listing must paint ungutted: {rows:?}"
    );

    // And the read-only key table is back: `Enter` descends again.
    press(&mut state, KeyCode::Enter);
    assert!(
        matches!(state.take_action(), Some(PendingAction::OpenPath(_))),
        "a re-list must restore the read-only explorer"
    );
}

/// While an edit is in progress the overlay paints the buffer, gutter and
/// all — not the listing underneath it.
#[test]
fn test_the_overlay_paints_the_buffer_once_editing_starts() {
    let dir = fixture("overlay");
    let mut state = app_state();
    state.install_listing(Listing::read(&dir), 1, true);

    let before = state.explore_rows(10);
    assert_eq!(before[1].text, "alpha.md", "{before:?}");

    press_char(&mut state, 'i');
    press_char(&mut state, 'X');
    let after = state.explore_rows(10);
    assert_eq!(after[1].text, "~ alpha.mdX", "{after:?}");
    assert_eq!(after[0].text, "  ../", "{after:?}");
}

/// `Esc` while typing puts the row back and leaves the buffer otherwise
/// alone; a second `Esc` then discards the buffer itself.
#[test]
fn test_esc_while_typing_reverts_only_the_row() {
    let dir = fixture("esc-row");
    let mut state = app_state();
    state.install_listing(Listing::read(&dir), 1, true);

    press_char(&mut state, 'i');
    for ch in "XYZ".chars() {
        press_char(&mut state, ch);
    }
    press(&mut state, KeyCode::Esc);
    let rows = state.explore_rows(10);
    assert_eq!(rows[1].text, "  alpha.md", "{rows:?}");

    // Still in the buffer, not back in the listing: `Enter` is refused.
    press(&mut state, KeyCode::Enter);
    assert!(state.take_action().is_none());
    assert!(status_text(&mut state).contains("unsaved edits"));
}

/// A control chord while typing is not a character. Without the guard,
/// `Ctrl-d` would put a `d` in a filename.
#[test]
fn test_a_control_chord_while_typing_is_not_a_character() {
    let dir = fixture("ctrl-chord");
    let mut state = app_state();
    state.install_listing(Listing::read(&dir), 1, true);

    press_char(&mut state, 'i');
    state.handle_key_event(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    let rows = state.explore_rows(10);
    assert_eq!(rows[1].text, "  alpha.md", "{rows:?}");
}

/// `q` quits from the buffer but is a character while typing a name — and
/// `Ctrl-c` quits from either, which is what makes the first half safe.
#[test]
fn test_q_is_a_character_while_typing_and_ctrl_c_always_quits() {
    let dir = fixture("quit-keys");
    let mut state = app_state();
    state.install_listing(Listing::read(&dir), 1, true);

    press_char(&mut state, 'i');
    assert!(
        !state.handle_key_event(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        "`q` must be a character while typing a name"
    );
    assert_eq!(state.explore_rows(10)[1].text, "~ alpha.mdq");
    assert!(
        state.handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        "Ctrl-c must quit even mid-name"
    );

    let mut state = app_state();
    state.install_listing(Listing::read(&dir), 1, true);
    press_char(&mut state, 'd');
    assert!(
        state.handle_key_event(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        "`q` quits from the buffer itself"
    );
}

// ============================================ adversarial review regressions
//
// Every test below reproduces a way an adversarial security review destroyed
// a reader's file on disk, or reported a lie about having done so. They were
// written *before* the fixes and watched to fail; each names the finding it
// pins so the next person to weaken a guard learns which demonstration comes
// back.

/// Every one of `wanted`'s byte strings is still somewhere in `dir`.
///
/// The oracle for the two-renames-collide findings, and it is deliberately
/// *not* "the directory looks like this": on a case-sensitive filesystem both
/// targets are legal names and the edit genuinely applies, while on a folding
/// one it must be refused. What may never differ between the two is that a
/// file's bytes went missing without a word.
fn assert_contents_survive(dir: &Path, wanted: &[&str], note: &str) {
    let after = snapshot(dir);
    for want in wanted {
        assert!(
            after.values().any(|value| value == want),
            "{note}: `{want}` is gone — {after:?}"
        );
    }
}

/// **F1, case half.** `a.md` → `Target.md` and `b.md` → `target.md` in one
/// write. Byte-exact name comparison says the two targets differ; a
/// case-folding filesystem says they do not, and the second `rename` lands on
/// the first one's result and replaces it.
///
/// Demonstrated: the directory afterwards held only `Target.md` with `BBB`,
/// `destructive()` was empty, and the summary said "applied 2 operations".
#[test]
fn test_f1_two_renames_colliding_by_case_never_lose_a_file() {
    let dir = scratch_dir("f1-case");
    fs::write(dir.join("a.md"), "AAA\n").expect("write a");
    fs::write(dir.join("b.md"), "BBB\n").expect("write b");

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_lines(&mut lines, &[("a.md", "Target.md"), ("b.md", "target.md")]);

    match diff(&listing, &lines) {
        Err(_) => assert_contents_survive(&dir, &["AAA\n", "BBB\n"], "a refused diff"),
        Ok(plan) => {
            plan.apply();
            assert_contents_survive(&dir, &["AAA\n", "BBB\n"], "an applied plan");
        }
    }
}

/// **F1, normalization half — the nastier one.** NFC `café.md` and NFD
/// `café.md` are visually identical, so no reader can see the collision, and
/// no case fold catches it either: `to_lowercase` leaves both unchanged and
/// unequal. Only the filesystem knows they are one name.
///
/// This is therefore the test for the *apply-time* layer specifically. It is
/// platform-adaptive rather than platform-skipped: where the filesystem keeps
/// the two names apart both files must exist afterwards, and where it folds
/// them the second operation must be refused — in both cases with no byte
/// lost.
#[test]
fn test_f1_two_renames_colliding_by_unicode_normalization_never_lose_a_file() {
    let dir = scratch_dir("f1-nfd");
    let nfc = "caf\u{e9}.md";
    let nfd = "cafe\u{301}.md";
    fs::write(dir.join("a.md"), "AAA\n").expect("write a");
    fs::write(dir.join("b.md"), "BBB\n").expect("write b");

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_lines(&mut lines, &[("a.md", nfc), ("b.md", nfd)]);

    match diff(&listing, &lines) {
        Err(_) => assert_contents_survive(&dir, &["AAA\n", "BBB\n"], "a refused diff"),
        Ok(plan) => {
            plan.apply();
            assert_contents_survive(&dir, &["AAA\n", "BBB\n"], "an applied plan");
        }
    }
}

/// **F1 for directories**, which no hard link can serve: `link(2)` answers
/// `EPERM` for a directory, so the atomic primitive that closes the file case
/// is unavailable and the guard has to be the check immediately before the
/// syscall.
///
/// **`one` is empty on purpose**, and that is the whole reproduction. POSIX
/// `rename` refuses to replace a *non-empty* directory (`ENOTEMPTY`), so a
/// fixture where both directories have contents is accidentally safe and
/// proves nothing. It will happily replace an empty one — so the collision
/// destroys `one` outright and the reader is told two operations applied.
#[test]
fn test_f1_two_directory_renames_colliding_never_lose_a_directory() {
    let dir = scratch_dir("f1-dirs");
    let nfc = "caf\u{e9}";
    let nfd = "cafe\u{301}";
    fs::create_dir(dir.join("one")).expect("create one");
    fs::create_dir(dir.join("two")).expect("create two");
    fs::write(dir.join("two").join("inside-two"), "2\n").expect("write inside two");

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_lines(&mut lines, &[("one/", nfc), ("two/", nfd)]);

    match diff(&listing, &lines) {
        Err(_) => {}
        Ok(plan) => {
            plan.apply();
        }
    }
    let after = snapshot(&dir);
    assert_eq!(
        after.len(),
        2,
        "one directory replaced the other: {after:?}"
    );
    assert_contents_survive(
        &dir,
        &["<dir []>", "<dir [inside-two]>"],
        "two directory renames",
    );
}

/// **F2.** The `stat`-then-`rename` window, driven by an in-process racer that
/// creates the target with `O_EXCL` while `apply` is running. Measured at 1296
/// clobbers in 3000 attempts before the fix — not a theoretical window.
///
/// The assertion never flakes in the safe direction: whenever the racer's
/// `create_new` succeeded, its bytes must still be there afterwards. A round
/// the racer loses asserts nothing and costs nothing.
#[test]
fn test_f2_a_racing_create_of_a_rename_target_is_never_clobbered() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let root = scratch_dir("f2-race");
    let mut clobbers = 0;
    let mut racer_wins = 0;
    for round in 0..400 {
        let dir = root.join(format!("round-{round}"));
        fs::create_dir(&dir).expect("create round dir");
        fs::write(dir.join("a.md"), "SOURCE\n").expect("write source");

        let listing = Listing::read(&dir);
        let mut lines = lines_of(&listing);
        rename_line(&mut lines, "a.md", "z.md");
        let plan = diff(&listing, &lines).expect("legal");

        let target = dir.join("z.md");
        let go = Arc::new(AtomicBool::new(false));
        let racer = {
            let go = Arc::clone(&go);
            let target = target.clone();
            std::thread::spawn(move || {
                while !go.load(Ordering::Acquire) {
                    std::hint::spin_loop();
                }
                for _ in 0..4096 {
                    let made = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&target)
                        .map(|mut file| {
                            use std::io::Write as _;
                            let _ = file.write_all(b"VICTIM\n");
                        });
                    if made.is_ok() {
                        return true;
                    }
                }
                false
            })
        };
        go.store(true, Ordering::Release);
        plan.apply();
        let won = racer.join().expect("racer thread");

        if won {
            racer_wins += 1;
            if fs::read_to_string(&target).unwrap_or_default() != "VICTIM\n" {
                clobbers += 1;
            }
        }
    }
    assert_eq!(
        clobbers, 0,
        "a rename replaced a file another process had just created \
         ({clobbers} of {racer_wins} contested rounds)"
    );
}

/// **F3.** A delete's worst case is not a failed remove. Between `w` and `y`
/// — a *human-scale* window, because the confirmation prompt sits inside it —
/// the name can come to hold a file the reader never saw, and the plan removes
/// that one instead.
///
/// **Known-failing on Linux, accepted as debt for 0.3.0 — see #5.** The
/// regression this pins is genuinely not held there: the impostor presents the
/// same `(dev, ino)` as the file it replaced, so the identity comparison sees
/// a match and the delete proceeds. The unit-level half of this, in
/// `explore::tests`, is ignored for the same reason and says more about it.
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "known-failing on Linux: (dev, ino) is reused across delete-and-recreate — see #5"
)]
#[test]
fn test_f3_a_delete_refuses_a_name_that_now_holds_a_different_file() {
    let dir = scratch_dir("f3-swap");
    fs::write(dir.join("doomed.md"), "THE READER'S FILE\n").expect("write doomed");

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    remove_line(&mut lines, "doomed.md");
    let plan = diff(&listing, &lines).expect("legal");

    // The window: the reader is still reading the confirmation.
    fs::remove_file(dir.join("doomed.md")).expect("remove the original");
    fs::write(dir.join("doomed.md"), "SOMEONE ELSE'S FILE\n").expect("write the impostor");

    let results = plan.apply();
    assert!(
        results.iter().all(|(_, result)| result.is_err()),
        "a delete must refuse a file it never listed: {results:?}"
    );
    assert_contents_survive(&dir, &["SOMEONE ELSE'S FILE\n"], "a swapped delete target");
    let summary = apply_summary(&results);
    assert!(
        !summary.starts_with("applied 1 operation"),
        "the summary must not claim a delete that was refused: {summary}"
    );
}

/// **F5, delete half.** Replacing the listed directory itself with a symlink
/// carries every operation out of the directory the reader was looking at,
/// which is the "no operation targets a path outside the listed directory"
/// invariant failing under TOCTOU.
#[test]
fn test_f5_a_delete_refuses_when_the_listed_directory_became_a_symlink() {
    let root = scratch_dir("f5-delete");
    let dir = root.join("dir");
    let elsewhere = root.join("elsewhere");
    fs::create_dir(&dir).expect("create dir");
    fs::create_dir(&elsewhere).expect("create elsewhere");
    fs::write(dir.join("doomed.md"), "MINE\n").expect("write mine");
    fs::write(elsewhere.join("doomed.md"), "NOT MINE\n").expect("write not mine");

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    remove_line(&mut lines, "doomed.md");
    let plan = diff(&listing, &lines).expect("legal");

    fs::remove_dir_all(&dir).expect("remove the listed directory");
    std::os::unix::fs::symlink(&elsewhere, &dir).expect("swap in a symlink");

    let results = plan.apply();
    assert!(
        results.iter().all(|(_, result)| result.is_err()),
        "a swapped directory must refuse every operation: {results:?}"
    );
    assert_contents_survive(&elsewhere, &["NOT MINE\n"], "a swapped listed directory");
}

/// **F5, create half** — the one the per-target identity check cannot cover,
/// because a create's target does not exist yet and so has no identity to
/// compare. Only the directory's own identity can refuse this.
#[test]
fn test_f5_a_create_refuses_when_the_listed_directory_became_a_symlink() {
    let root = scratch_dir("f5-create");
    let dir = root.join("dir");
    let elsewhere = root.join("elsewhere");
    fs::create_dir(&dir).expect("create dir");
    fs::create_dir(&elsewhere).expect("create elsewhere");
    fs::write(dir.join("keep.md"), "KEEP\n").expect("write keep");

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    add_line(&mut lines, "planted.md");
    let plan = diff(&listing, &lines).expect("legal");

    fs::remove_dir_all(&dir).expect("remove the listed directory");
    std::os::unix::fs::symlink(&elsewhere, &dir).expect("swap in a symlink");

    let results = plan.apply();
    assert!(
        results.iter().all(|(_, result)| result.is_err()),
        "a swapped directory must refuse every operation: {results:?}"
    );
    assert!(
        !elsewhere.join("planted.md").exists(),
        "a create escaped the listed directory: {:?}",
        snapshot(&elsewhere)
    );
}

/// **F4.** A 48-key modifier sweep from a freshly-armed gate found every
/// `Char('y')` under every modifier queued the apply. The confirmation for an
/// irreversible operation must require the bare key.
#[test]
fn test_f4_only_a_bare_y_confirms_and_no_modified_y_does() {
    let dir = fixture("f4-modifiers");
    let before = snapshot(&dir);

    let mut state = app_state();
    state.install_listing(Listing::read(&dir), 1, true);
    press_char(&mut state, 'd'); // mark alpha.md for removal
    press_char(&mut state, 'w');
    assert!(status_text(&mut state).contains("DELETE 1 (alpha.md)"));

    let modifiers = [
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SHIFT,
        KeyModifiers::SUPER,
        KeyModifiers::HYPER,
        KeyModifiers::META,
    ];
    for bits in 1u8..(1 << 6) {
        let mut mods = KeyModifiers::NONE;
        for (index, modifier) in modifiers.iter().enumerate() {
            if bits & (1 << index) != 0 {
                mods |= *modifier;
            }
        }
        for code in [KeyCode::Char('y'), KeyCode::Char('Y')] {
            state.handle_key_event(KeyEvent::new(code, mods));
            assert!(
                state.take_action().is_none(),
                "{code:?} with {mods:?} must not confirm an irreversible delete"
            );
            assert!(
                status_text(&mut state).contains("[y/n]"),
                "{code:?} with {mods:?} must leave the question up"
            );
            assert_eq!(snapshot(&dir), before);
        }
    }

    press_char(&mut state, 'y');
    assert!(
        matches!(state.take_action(), Some(PendingAction::ApplyEdits(_))),
        "a bare `y` is still the one key that confirms"
    );
}

/// **F7.** A case-only rename that the filesystem turns into a no-op must not
/// be reported as an applied operation.
///
/// Platform-adaptive, and both branches assert something: where two hard links
/// can differ by case alone the rename does nothing and must say so, and where
/// they cannot the rename really re-cases the directory entry and `read_dir`
/// is what proves it — `exists()` cannot, because on a folding filesystem the
/// old name still resolves.
#[test]
fn test_f7_a_case_only_rename_reports_success_only_when_it_really_happened() {
    let dir = scratch_dir("f7-case-noop");
    fs::write(dir.join("readme.md"), "R\n").expect("write readme");
    let hardlinked_pair = fs::hard_link(dir.join("readme.md"), dir.join("README.md")).is_ok();

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "readme.md", "README.md");

    let plan = match diff(&listing, &lines) {
        Ok(plan) => plan,
        Err(error) => {
            assert!(
                hardlinked_pair,
                "a plain case-only rename must be a legal edit, got {error:?}"
            );
            return;
        }
    };
    let results = plan.apply();
    let summary = apply_summary(&results);
    let names: Vec<OsString> = fs::read_dir(&dir)
        .expect("read dir")
        .map(|entry| entry.expect("entry").file_name())
        .collect();

    if hardlinked_pair {
        assert!(
            !summary.starts_with("applied 1 operation"),
            "a rename that did nothing must not report success: {summary}"
        );
        assert!(
            names.contains(&OsString::from("readme.md")),
            "nothing moved, so both entries are still there: {names:?}"
        );
    } else {
        assert_eq!(summary, "applied 1 operation", "{names:?}");
        assert!(
            names.contains(&OsString::from("README.md"))
                && !names.contains(&OsString::from("readme.md")),
            "the directory entry itself must have been re-cased: {names:?}"
        );
    }
}

// ------------------------------------------- what the reader is actually told

/// Every refusal's rendered sentence, pinned.
///
/// **Added because a mutation sweep found the gap**, and it is not a
/// cosmetic one: the tests above match on `EditError`'s *fields*, so
/// `NameFault`'s whole `Display` impl could be replaced with `Ok(())` — every
/// refusal message losing its reason — and all of them still passed. A
/// refusal a reader cannot read is a refusal that looks like a bug in the
/// editor.
#[test]
fn test_every_refusal_says_why_in_words() {
    let cases: Vec<(EditError, &str)> = vec![
        (
            EditError::MalformedRow { line: 0 },
            "line 1: not a buffer row",
        ),
        (
            EditError::UnknownRow { line: 2, id: 9 },
            "line 3: no entry 9 in this listing",
        ),
        (
            EditError::DuplicateRow { line: 4, id: 1 },
            "line 5: entry 1 is named twice",
        ),
        (
            EditError::ParentRow { line: 0 },
            "line 1: the `../` row cannot be edited",
        ),
        (
            EditError::InconsistentListing {
                name: "odd".to_string(),
            },
            "`odd`: this is not a plain reading of one directory and cannot be edited",
        ),
        (
            EditError::NameTaken {
                line: 1,
                name: "taken.md".to_string(),
                existing: "taken.md".to_string(),
            },
            "line 2: `taken.md` already exists here — move it away in a separate write first",
        ),
        // The folded halves, which must not claim the reader's own spelling
        // is what is sitting there: `target.md` is not on disk, `Target.md`
        // is, and a reader sent looking for the wrong one finds nothing.
        (
            EditError::NameTaken {
                line: 1,
                name: "target.md".to_string(),
                existing: "Target.md".to_string(),
            },
            "line 2: `target.md` and `Target.md`, which already exists here, are one name on this \
             filesystem",
        ),
        (
            EditError::DuplicateName {
                name: "same.md".to_string(),
                other: "same.md".to_string(),
            },
            "two rows both name `same.md`",
        ),
        (
            EditError::DuplicateName {
                name: "target.md".to_string(),
                other: "Target.md".to_string(),
            },
            "`target.md` and `Target.md` are one name on this filesystem, and two rows ask for both",
        ),
        (
            EditError::NonUtf8Rename {
                line: 0,
                shown: "caf\u{fffd}.md".to_string(),
            },
            "line 1: `caf\u{fffd}.md` is not valid text on disk and cannot be renamed from here",
        ),
        (
            EditError::DirectoryCreate {
                line: 3,
                name: "d/".to_string(),
            },
            "line 4: `d/` — this phase creates files, not directories",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }

    // And each name fault's own clause, which rides inside `IllegalName`.
    for (fault, expected) in [
        (
            NameFault::Blank,
            "a name cannot be empty or only whitespace",
        ),
        (
            NameFault::Separator,
            "a name cannot contain a path separator",
        ),
        (NameFault::Dot, "`.` and `..` are not names"),
        (NameFault::Nul, "a name cannot contain a NUL byte"),
        (NameFault::Newline, "a name cannot contain a line break"),
        (
            NameFault::Escapes,
            "that name does not stay inside this directory",
        ),
    ] {
        let error = EditError::IllegalName {
            line: 6,
            name: "x".to_string(),
            fault,
        };
        assert_eq!(error.to_string(), format!("line 7: `x` — {expected}"));
    }
}

/// The confirmation sentence, **whole**, with three different counts.
///
/// Also added off a mutation sweep. The gate tests above assert that the
/// question names the doomed file and asks `[y/n]`, which is what matters
/// most — and left the rename and create counts free to be any number at all:
/// with one of each, `creates = first_delete - renames` and
/// `first_delete + renames` agree, and every `> 0` guard could be flipped
/// without a single failure. Two, three and seven cannot agree with anything.
#[test]
fn test_the_confirmation_counts_every_operation_and_names_the_doomed() {
    let dir = scratch_dir("confirmation");
    for name in ["r1.md", "r2.md"] {
        fs::write(dir.join(name), "rename me\n").expect("write");
    }
    for index in 1..=7 {
        fs::write(dir.join(format!("d{index}.md")), "delete me\n").expect("write");
    }
    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_lines(&mut lines, &[("r1.md", "n1.md"), ("r2.md", "n2.md")]);
    for name in ["c1.md", "c2.md", "c3.md"] {
        add_line(&mut lines, name);
    }
    for index in 1..=7 {
        remove_line(&mut lines, &format!("d{index}.md"));
    }

    let plan = diff(&listing, &lines).expect("legal");
    assert_eq!(
        plan.confirmation(),
        "apply 2 renames, 3 creates, \
         DELETE 7 (d1.md, d2.md, d3.md, d4.md, d5.md, d6.md and 1 more)? [y/n]"
    );

    // Exactly at the naming limit, nothing is elided.
    let mut six = lines_of(&listing);
    for index in 1..=6 {
        remove_line(&mut six, &format!("d{index}.md"));
    }
    assert_eq!(
        diff(&listing, &six).expect("legal").confirmation(),
        "apply DELETE 6 (d1.md, d2.md, d3.md, d4.md, d5.md, d6.md)? [y/n]"
    );

    // The singular of all three.
    let mut one = lines_of(&listing);
    rename_line(&mut one, "r1.md", "n1.md");
    add_line(&mut one, "c1.md");
    remove_line(&mut one, "d1.md");
    assert_eq!(
        diff(&listing, &one).expect("legal").confirmation(),
        "apply 1 rename, 1 create, DELETE 1 (d1.md)? [y/n]"
    );

    // Renames alone: no create clause, no delete clause.
    let mut renames_only = lines_of(&listing);
    rename_lines(&mut renames_only, &[("r1.md", "n1.md"), ("r2.md", "n2.md")]);
    assert_eq!(
        diff(&listing, &renames_only).expect("legal").confirmation(),
        "apply 2 renames? [y/n]"
    );

    // Creates alone.
    let mut creates_only = lines_of(&listing);
    add_line(&mut creates_only, "c1.md");
    assert_eq!(
        diff(&listing, &creates_only).expect("legal").confirmation(),
        "apply 1 create? [y/n]"
    );
}

/// A truncated listing's confirmation says what it ignored, in both numbers.
#[test]
fn test_the_confirmation_reports_what_truncation_suppressed() {
    let dir = fixture("confirmation-capped");
    let honest = Listing::read(&dir);
    let capped = Listing::from_entries(honest.dir().to_path_buf(), honest.entries().to_vec(), true);

    let mut two = lines_of(&honest);
    remove_line(&mut two, "beta.md");
    remove_line(&mut two, "gamma.md");
    add_line(&mut two, "fresh.md");
    assert_eq!(
        diff(&capped, &two).expect("legal").confirmation(),
        "apply 1 create? [y/n]  (2 removed rows ignored: the listing was capped, \
         so absence cannot mean removal)"
    );

    let mut one = lines_of(&honest);
    remove_line(&mut one, "beta.md");
    assert_eq!(
        diff(&capped, &one).expect("legal").confirmation(),
        "apply nothing? [y/n]  (1 removed row ignored: the listing was capped, \
         so absence cannot mean removal)"
    );

    // And an edit that asks for nothing at all, with nothing suppressed.
    assert_eq!(
        diff(&honest, &lines_of(&honest))
            .expect("legal")
            .confirmation(),
        "apply nothing? [y/n]"
    );
}

/// Files whose real names look exactly like gutter markers round-trip to an
/// empty plan.
///
/// A review noted that `- baz` on screen could be a row marked for removal or
/// a file honestly called `- baz`, and asked whether the ambiguity was only
/// cosmetic. It is: the marker is prepended at paint time and never stored in
/// the row's text, so what the diff sees is the name and nothing else. This is
/// the test that says so, and it is the one the `MARK_UNCHANGED` comment in
/// `explore.rs` points at — written because that comment cited a check a
/// reviewer had run by hand and never committed.
#[test]
fn test_the_two_column_gutter_cannot_be_confused_with_a_filename() {
    let dir = scratch_dir("gutter-lookalikes");
    for name in ["~ foo", "+ bar", "- baz", "  quux"] {
        fs::write(dir.join(name), format!("{name}\n")).expect("write lookalike");
    }
    let before = snapshot(&dir);

    let listing = Listing::read(&dir);
    let buffer = EditBuffer::new(&listing);

    // What the reader sees: every real name sits one two-cell gutter to the
    // right, whatever it is spelled.
    let painted: Vec<String> = buffer.rows(10, 0).into_iter().map(|row| row.text).collect();
    for name in ["~ foo", "+ bar", "- baz", "  quux"] {
        assert!(
            painted.contains(&format!("  {name}")),
            "`{name}` must paint under the unchanged marker: {painted:?}"
        );
    }

    let plan = diff(&listing, &buffer.lines()).expect("an untouched buffer is legal");
    assert!(
        plan.is_empty(),
        "a buffer nobody edited must ask for nothing: {:?}",
        plan.operations()
    );
    plan.apply();
    assert_eq!(snapshot(&dir), before);
}

/// DW-4.5's actual words: refused **before any operation runs**. Not "the
/// colliding operation fails".
///
/// **Every other pre-flight test uses a one-operation plan**, where those two
/// are indistinguishable — the create fails either way and the directory ends
/// up the same. A layer audit reverted the pre-flight's whole collision loop
/// and all three of them stayed green. This one puts a rename the plan would
/// happily perform *ahead* of the collision, so the assertion that
/// `alpha.md` is still called `alpha.md` is the batch guarantee itself.
#[test]
fn test_dw_4_5_a_late_collision_stops_the_earlier_operations_running() {
    let dir = fixture("dw45-late-collision");
    let before = snapshot(&dir);

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    rename_line(&mut lines, "alpha.md", "renamed.md");
    add_line(&mut lines, "squatted.md");
    let plan = diff(&listing, &lines).expect("legal at diff time");
    // Renames are stored before creates, so the rename really is the earlier
    // operation and would already have run by the time the create failed.
    assert_eq!(plan.len(), 2, "{:?}", plan.operations());
    assert!(
        matches!(plan.operations()[0], Operation::Rename { .. }),
        "{:?}",
        plan.operations()
    );

    fs::write(dir.join("squatted.md"), "SQUATTER\n").expect("write the collider");
    let mut expected = before.clone();
    expected.insert(OsString::from("squatted.md"), "SQUATTER\n".to_string());

    let results = plan.apply();
    assert!(
        results.iter().all(|(_, result)| result.is_err()),
        "a collision must refuse the whole plan: {results:?}"
    );
    assert_eq!(
        snapshot(&dir),
        expected,
        "the rename ran even though a later operation was refused"
    );
}

/// The same batch guarantee for the other thing the pre-flight refuses: a
/// listed directory that is no longer the one that was read.
///
/// Asserted on the *sentence*, because that is the only thing that separates
/// the batch refusal from the per-operation one. Without the pre-flight's
/// directory check every operation still fails — the first on the directory,
/// the rest as "not attempted" — so a test that only counted failures could
/// not tell the two apart, and the same layer audit found exactly that.
#[test]
fn test_f5_a_swapped_directory_refuses_the_batch_and_not_merely_each_operation() {
    let root = scratch_dir("f5-batch");
    let dir = root.join("dir");
    let elsewhere = root.join("elsewhere");
    fs::create_dir(&dir).expect("create dir");
    fs::create_dir(&elsewhere).expect("create elsewhere");
    fs::write(dir.join("keep.md"), "KEEP\n").expect("write keep");

    let listing = Listing::read(&dir);
    let mut lines = lines_of(&listing);
    add_line(&mut lines, "one.md");
    add_line(&mut lines, "two.md");
    let plan = diff(&listing, &lines).expect("legal");
    assert_eq!(plan.len(), 2, "{:?}", plan.operations());

    fs::remove_dir_all(&dir).expect("remove the listed directory");
    std::os::unix::fs::symlink(&elsewhere, &dir).expect("swap in a symlink");

    let results = plan.apply();
    for (op, result) in &results {
        let reason = result
            .as_ref()
            .expect_err("every operation must fail")
            .to_string();
        assert!(
            reason.contains("no operation ran"),
            "{op} reported `{reason}`, which is a per-operation refusal, not a batch one"
        );
    }
    assert!(
        snapshot(&elsewhere).is_empty(),
        "{:?}",
        snapshot(&elsewhere)
    );
}

/// The buffer's own notice row, through the shared windowing — the three
/// operators that survived a mutation sweep while `Listing::rows`'s identical
/// copies were killed.
///
/// The arithmetic is unit-tested at `explore::tests::test_overlay_window_arithmetic`;
/// this is the half that proves `EditBuffer::rows` actually routes through it,
/// notice and all, rather than growing a second copy again.
#[test]
fn test_a_capped_buffer_windows_its_notice_row_like_the_listing_does() {
    let dir = scratch_dir("buffer-notice-window");
    for index in 0..10 {
        fs::write(dir.join(format!("file-{index}.md")), "x\n").expect("write");
    }
    let entries = Listing::read(&dir).entries().to_vec();
    // Truncated, so the buffer carries a notice row of its own.
    let capped = Listing::from_entries(dir.clone(), entries, true);
    let buffer = EditBuffer::new(&capped);
    assert_eq!(buffer.len(), 11, "`../` plus ten files");

    // `height - notice`: one row of viewport is all notice, no entries.
    let rows = buffer.rows(1, 0);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0].text.starts_with("listing capped"), "{rows:?}");

    // The notice never costs more than its one row, and never lets the frame
    // overflow the viewport.
    for height in 1..10u16 {
        let rows = buffer.rows(height, 3);
        assert!(
            rows.len() <= usize::from(height),
            "height {height} painted {} rows",
            rows.len()
        );
    }

    // `window / 2`: with the notice taking one row of five, four entry rows
    // remain and the cursor sits mid-window rather than at its top. Row 5 of
    // eleven is far from either end, so nothing clamps and the centring is
    // the only thing deciding where the cursor lands.
    let rows = buffer.rows(5, 5);
    assert_eq!(rows.len(), 5, "{rows:?}");
    assert!(rows[0].text.starts_with("listing capped"), "{rows:?}");
    let selected: Vec<&str> = rows
        .iter()
        .filter(|row| row.style == RowStyle::Selected)
        .map(|row| row.text.as_str())
        .collect();
    assert_eq!(selected.len(), 1, "{rows:?}");
    let at = rows
        .iter()
        .position(|row| row.style == RowStyle::Selected)
        .expect("a cursor");
    assert_eq!(
        at, 3,
        "the cursor must be centred in the entry rows: {rows:?}"
    );
}

/// An empty listing that still has something to say: nonzero budget, no rows.
///
/// This is the shape that underflowed `Listing::rows` before its guard's `||`
/// was understood, and the buffer held an untested copy of the same guard.
#[test]
fn test_an_empty_capped_buffer_paints_its_notice_and_nothing_else() {
    let dir = scratch_dir("buffer-notice-empty");
    let capped = Listing::from_entries(dir, Vec::new(), true);
    let buffer = EditBuffer::new(&capped);
    assert!(buffer.is_empty());

    let rows = buffer.rows(10, 0);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0].text.starts_with("listing capped"), "{rows:?}");
    // And a stale selection over an empty buffer must not panic either.
    assert_eq!(buffer.rows(10, 99).len(), 1);
}

/// The dirty indicator's four counts, each a **different number**.
///
/// Written after a first attempt at this failed to catch anything: the
/// existing marker test happens to have two renamed rows and two unchanged
/// ones, so `tally`'s `row.text != row.seeded` could become `==` — counting
/// every untouched row as renamed — and still print "2 renamed". A fixture
/// where the counts collide cannot detect a swap between them, so this one has
/// 1 renamed, 2 new, 3 removed and 4 untouched, and no two are equal.
#[test]
fn test_the_dirty_indicator_counts_each_kind_of_change() {
    let dir = scratch_dir("indicator-counts");
    for name in ["a.md", "b.md", "c.md", "d.md", "e.md", "f.md", "g.md"] {
        fs::write(dir.join(name), "x\n").expect("write");
    }
    let listing = Listing::read(&dir);
    let mut buffer = EditBuffer::new(&listing);
    // Rows: 0 `../`, 1 a.md, 2 b.md, 3 c.md, 4 d.md, 5 e.md, 6 f.md, 7 g.md.
    assert_eq!(buffer.len(), 8, "{:?}", buffer.lines());

    buffer.push_char(1, 'X'); // one rename
    for row in [2, 3, 4] {
        buffer.toggle_removed(row); // three removals
    }
    // Two additions, appended last so no index above shifts under us.
    buffer.insert_new(7);
    buffer.insert_new(8);

    // Untouched: `../`, e.md, f.md, g.md — four of them, and none may be
    // counted as anything else.
    assert_eq!(
        buffer.indicator(),
        "[+] 1 renamed, 2 new, 3 removed — w writes, Esc discards"
    );
}

/// The gate must be *visible* whenever it is armed, not merely armed.
///
/// A transient status message set before the plan was built used to win the
/// status row for its whole 100-frame TTL while the confirmation sat behind
/// it. `w`, `n`, `w` is the shortest way there: the `n` leaves "write
/// cancelled: nothing on disk was changed" on the row, the second `w` re-arms
/// the same DELETE behind it, and a bare `y` ran the delete off a row that
/// said nothing had been changed. The rollback story for this phase is that
/// every destructive operation is named and counted before anything runs —
/// there is no trash and no undo — so a question the reader cannot see is not
/// a lesser bug than a question that is not asked.
#[test]
fn test_dw_4_3_an_armed_gate_is_never_hidden_by_a_stale_message() {
    let dir = fixture("dw43-visible-gate");
    let before = snapshot(&dir);

    let mut state = app_state();
    state.install_listing(Listing::read(&dir), 1, true);
    press_char(&mut state, 'd'); // mark alpha.md for removal
    press_char(&mut state, 'w');
    assert!(
        status_text(&mut state).contains("DELETE 1 (alpha.md)"),
        "the first arming must show the question"
    );

    press_char(&mut state, 'n'); // leaves a transient with its full TTL
    press_char(&mut state, 'w'); // re-arms the same plan behind it

    let row = status_text(&mut state);
    assert!(
        row.contains("DELETE 1 (alpha.md)") && row.contains("[y/n]"),
        "a re-armed gate must name its delete, not report the last cancel: {row:?}"
    );
    assert_eq!(snapshot(&dir), before, "arming still writes nothing");
}

/// And the stale message must not outlive the gate either: resolving the
/// confirmation may not hand the row back to a message describing the edit
/// that came before it.
#[test]
fn test_dw_4_3_arming_the_gate_retires_the_previous_message() {
    let dir = fixture("dw43-retired-message");

    let mut state = app_state();
    state.install_listing(Listing::read(&dir), 1, true);
    press_char(&mut state, 'd');
    press_char(&mut state, 'w');
    press_char(&mut state, 'n');
    press_char(&mut state, 'w'); // re-arm, retiring the cancel
    press_char(&mut state, 'y'); // resolve the gate

    assert!(
        matches!(state.take_action(), Some(PendingAction::ApplyEdits(_))),
        "sanity: `y` still confirms"
    );
    let row = status_text(&mut state);
    assert!(
        !row.contains("nothing on disk was changed"),
        "a cancel from before the gate must not reappear after it: {row:?}"
    );
}

/// And the precedence is a property of the row, not of the two call sites
/// that happen to get the order right today.
///
/// [`AppState::build_edit_plan`] clearing the row is what fixes the `w`, `n`,
/// `w` sequence, but on its own it only fixes *that* sequence: any future
/// caller that sets a transient message while a plan is armed would hide the
/// question again, and no test above would notice. `set_status` is public, so
/// this test is that caller. The rule it pins is the one the phase's safety
/// story rests on — while a plan is waiting, the row says what the plan will
/// destroy, whatever else wanted to be there.
#[test]
fn test_dw_4_3_a_pending_confirmation_outranks_any_transient_message() {
    let dir = fixture("dw43-gate-precedence");
    let before = snapshot(&dir);

    let mut state = app_state();
    state.install_listing(Listing::read(&dir), 1, true);
    press_char(&mut state, 'd'); // mark alpha.md for removal
    press_char(&mut state, 'w'); // arm the gate

    state.set_status(StatusMessage::new("a message from somewhere else"));

    let row = status_text(&mut state);
    assert!(
        row.contains("DELETE 1 (alpha.md)") && row.contains("[y/n]"),
        "an armed gate must outrank a transient set after it: {row:?}"
    );

    // The suppressed message must not be waiting to take the row back, either.
    press_char(&mut state, 'n');
    press_char(&mut state, 'w');
    assert!(
        status_text(&mut state).contains("[y/n]"),
        "and it must not resurface across a cancel and a re-arm"
    );
    assert_eq!(snapshot(&dir), before, "none of this writes anything");
}

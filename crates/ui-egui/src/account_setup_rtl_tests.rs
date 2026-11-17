use super::*;

#[test]
fn server_row_reads_from_the_language_edge() {
    assert_eq!(server_row_layout(false).main_dir(), egui::Direction::LeftToRight);
    assert_eq!(server_row_layout(true).main_dir(), egui::Direction::RightToLeft);
}

#[test]
fn encryption_segments_round_the_physical_outer_edges() {
    assert_eq!(encryption_corners(0, 3, false), (4, 0));
    assert_eq!(encryption_corners(1, 3, false), (0, 0));
    assert_eq!(encryption_corners(2, 3, false), (0, 4));
    assert_eq!(encryption_corners(0, 3, true), (0, 4));
    assert_eq!(encryption_corners(1, 3, true), (0, 0));
    assert_eq!(encryption_corners(2, 3, true), (4, 0));
    assert_eq!(encryption_corners(0, 1, true), (4, 4));
}

use super::constants::{HEADER_H, PORT_HIT_R};
use super::geometry::{in_port_pos, out_port_pos};
use crate::ui::state::{NodeGraph, NodeId, NodeKind, Wire};
use eframe::egui;

pub(super) fn handle_interactions(
    ctx: &egui::Context,
    response: &egui::Response,
    rects: &[(NodeId, egui::Rect)],
    graph: &mut NodeGraph,
    canvas_rect: egui::Rect,
    zoom: f32,
) {
    let mouse_pos = response.hover_pos();
    let pressed = ctx.input(|i| i.pointer.primary_pressed());
    let released = ctx.input(|i| i.pointer.primary_released());
    let right_pressed = ctx.input(|i| i.pointer.secondary_pressed());
    let drag_delta = response.drag_delta();

    // Scroll wheel: zoom centered on cursor
    if response.hovered() {
        let scroll_y = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll_y != 0.0 {
            let factor: f32 = if scroll_y > 0.0 { 1.03 } else { 1.0 / 1.03 };
            let old_zoom = graph.canvas_zoom;
            let new_zoom = (old_zoom * factor).clamp(0.15, 5.0);
            if let Some(cursor) = mouse_pos {
                let origin = canvas_rect.min.to_vec2();
                // Keep the canvas point under the cursor fixed
                let cursor_canvas = (cursor.to_vec2() - origin - graph.canvas_offset) / old_zoom;
                graph.canvas_offset = cursor.to_vec2() - origin - cursor_canvas * new_zoom;
            }
            graph.canvas_zoom = new_zoom;
        }
    }

    // Middle-mouse pan (works regardless of other interactions)
    if response.hovered() {
        let middle_delta = ctx.input(|i| {
            if i.pointer.middle_down() {
                i.pointer.delta()
            } else {
                egui::Vec2::ZERO
            }
        });
        graph.canvas_offset += middle_delta;
    }

    // Apply drag delta to the node(s) being dragged
    if let Some((drag_id, _)) = graph.drag {
        if graph.selected.contains(&drag_id) {
            // Move every selected node together
            let selected: Vec<NodeId> = graph.selected.iter().copied().collect();
            for node in &mut graph.nodes {
                if selected.contains(&node.id) {
                    node.pos += drag_delta / zoom;
                }
            }
        } else if let Some(node) = graph.node_mut(drag_id) {
            node.pos += drag_delta / zoom;
        }
        if released {
            graph.drag = None;
        }
    } else if graph.marquee.is_some() {
        // Update the live end of the marquee; do NOT pan
        if let Some(mpos) = mouse_pos {
            if let Some(ref mut m) = graph.marquee {
                m.1 = mpos;
            }
        }
    }

    // Finalise marquee selection on release
    if released {
        if let Some((start, end)) = graph.marquee.take() {
            let sel_rect = egui::Rect::from_two_pos(start, end);
            if sel_rect.width() > 4.0 || sel_rect.height() > 4.0 {
                graph.selected.clear();
                for (id, rect) in rects {
                    if sel_rect.intersects(*rect) {
                        graph.selected.insert(*id);
                    }
                }
            } else {
                graph.selected.clear();
            }
        }
    }

    // Complete or cancel pending wire on mouse release
    if released {
        if let Some((from_id, from_port)) = graph.pending_wire.take() {
            if let Some(mpos) = mouse_pos {
                // Find which input port the cursor is over
                let mut new_wire: Option<Wire> = None;
                'find: for (node_id, rect) in rects {
                    if *node_id == from_id {
                        continue;
                    }
                    let in_count = graph
                        .nodes
                        .iter()
                        .find(|n| n.id == *node_id)
                        .map(|n| n.kind.input_count())
                        .unwrap_or(0);
                    for p in 0..in_count {
                        if mpos.distance(in_port_pos(*rect, p, zoom)) <= PORT_HIT_R * zoom {
                            new_wire = Some(Wire {
                                from_node: from_id,
                                from_port,
                                to_node: *node_id,
                                to_port: p,
                            });
                            break 'find;
                        }
                    }
                }
                if let Some(wire) = new_wire {
                    // Each input port accepts only one wire
                    graph
                        .wires
                        .retain(|w| !(w.to_node == wire.to_node && w.to_port == wire.to_port));
                    graph.wires.push(wire);
                }
            }
        }
    }

    // Start a new wire drag or node header drag on press
    if pressed && graph.drag.is_none() && graph.pending_wire.is_none() {
        if let Some(mpos) = mouse_pos {
            let mut started_wire = false;

            // Output port check (priority over header drag)
            'outer: for (node_id, rect) in rects {
                let out_count = graph
                    .nodes
                    .iter()
                    .find(|n| n.id == *node_id)
                    .map(|n| n.kind.output_count())
                    .unwrap_or(0);
                for p in 0..out_count {
                    if mpos.distance(out_port_pos(*rect, p, zoom)) <= PORT_HIT_R * zoom {
                        graph.pending_wire = Some((*node_id, p));
                        started_wire = true;
                        break 'outer;
                    }
                }
            }

            // Header drag
            if !started_wire {
                for (node_id, rect) in rects {
                    let header = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(rect.width(), HEADER_H * zoom),
                    );
                    if header.contains(mpos) {
                        graph.drag = Some((*node_id, egui::vec2(0.0, 0.0)));
                        break;
                    }
                }
            }

            // Clicked empty canvas → start marquee
            if !started_wire && graph.drag.is_none() {
                graph.marquee = Some((mpos, mpos));
            }
        }
    }

    // Right-click: delete non-permanent nodes
    if right_pressed {
        if let Some(mpos) = mouse_pos {
            let clicked_id = rects
                .iter()
                .find(|(_, rect)| rect.contains(mpos))
                .map(|(id, _)| *id);
            if let Some(id) = clicked_id {
                let permanent = graph
                    .nodes
                    .iter()
                    .find(|n| n.id == id)
                    .map(|n| matches!(n.kind, NodeKind::CsvSource | NodeKind::Output))
                    .unwrap_or(false);
                if !permanent {
                    graph.remove_node(id);
                }
            }
        }
    }

    // Escape cancels any in-progress interaction
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        graph.pending_wire = None;
        graph.drag = None;
    }
}

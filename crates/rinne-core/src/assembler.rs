//! The context assembler (`CONTEXT.md` §12).
//!
//! Builds each node's context packet from the blackboard. This is the hardest
//! component because no two workers share a context window. For a **harness**
//! worker it writes a thin packet and *pins file paths* — the worker reads the
//! repo itself. For an **API** worker it *inlines file contents* — the model
//! sees only what is sent. Get this right or workers talk past each other.

use std::path::{Path, PathBuf};

use crate::blackboard::Blackboard;
use crate::dag::{Node, Plan};
use crate::worker::{ContextPacket, InlinedFile, WorkerFamily};
use crate::{Result, BLACKBOARD_DIR};

/// Builds context packets against a plan and its blackboard.
pub struct ContextAssembler<'a> {
    blackboard: &'a Blackboard,
    plan: &'a Plan,
}

impl<'a> ContextAssembler<'a> {
    pub fn new(blackboard: &'a Blackboard, plan: &'a Plan) -> Self {
        Self { blackboard, plan }
    }

    /// Assemble the packet for `node`, shaped for the target worker `family`.
    ///
    /// `critique` carries an evaluator's feedback on loop-back (P5); pass `None`
    /// on the first attempt.
    pub fn build(
        &self,
        node: &Node,
        family: WorkerFamily,
        critique: Option<String>,
    ) -> Result<ContextPacket> {
        // The context sources are the same regardless of family: the plan's
        // pinned `@`-mentions plus this node's named input artifacts. Only the
        // *shaping* (paths vs. contents) differs.
        let mentioned = &self.plan.mentioned;
        let input_artifacts: Vec<String> = node
            .inputs
            .iter()
            .filter(|i| i.as_str() != "diff") // `diff` is a special pseudo-input
            .cloned()
            .collect();

        let mut packet = ContextPacket {
            critique,
            ..Default::default()
        };

        match family {
            WorkerFamily::Harness => {
                // Pin repo-relative mention paths, and the on-disk paths of input
                // artifacts (under .rinne/artifacts/) for the worker to read.
                for m in mentioned {
                    packet.pinned_paths.push(m.clone());
                }
                for name in &input_artifacts {
                    if self.blackboard.artifact_exists(name) {
                        packet
                            .pinned_paths
                            .push(artifact_rel_path(name));
                    }
                }
            }
            WorkerFamily::Api => {
                // Inline contents: the model sees only what we send.
                let workspace = self.blackboard.workspace();
                for m in mentioned {
                    if let Some(file) = read_inlined(workspace, m) {
                        packet.inlined_files.push(file);
                    }
                }
                for name in &input_artifacts {
                    if let Ok(contents) = self.blackboard.read_artifact(name) {
                        packet.inlined_files.push(InlinedFile {
                            path: artifact_rel_path(name),
                            contents,
                        });
                    }
                }
            }
        }

        Ok(packet)
    }
}

/// The workspace-relative path of a named artifact (e.g.
/// `.rinne/artifacts/design.md`), usable by a harness worker from the repo root.
fn artifact_rel_path(name: &str) -> PathBuf {
    Path::new(BLACKBOARD_DIR).join("artifacts").join(name)
}

/// Read a mentioned file's contents for inlining, resolving it against the
/// workspace. Returns `None` if it cannot be read (e.g. a directory or missing).
fn read_inlined(workspace: &Path, rel: &Path) -> Option<InlinedFile> {
    let abs = if rel.is_absolute() {
        rel.to_path_buf()
    } else {
        workspace.join(rel)
    };
    let contents = std::fs::read_to_string(&abs).ok()?;
    Some(InlinedFile {
        path: rel.to_path_buf(),
        contents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("rinne-asm-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn plan_with_mention() -> Plan {
        serde_json::from_value(serde_json::json!({
            "goal": "g",
            "mentioned": ["src/a.rs"],
            "nodes": [{
                "id": "n1",
                "role": "generator",
                "instruction": "do it",
                "needs": ["code-edit"],
                "inputs": ["design.md"]
            }]
        }))
        .unwrap()
    }

    #[test]
    fn harness_pins_paths_inlines_nothing() {
        let ws = temp_workspace("harness");
        std::fs::create_dir_all(ws.join("src")).unwrap();
        std::fs::write(ws.join("src/a.rs"), "fn a() {}").unwrap();
        let bb = Blackboard::open(&ws).unwrap();
        bb.write_artifact("design.md", "the design").unwrap();
        let plan = plan_with_mention();

        let asm = ContextAssembler::new(&bb, &plan);
        let packet = asm.build(&plan.nodes[0], WorkerFamily::Harness, None).unwrap();

        assert!(packet.inlined_files.is_empty());
        assert!(packet.pinned_paths.contains(&PathBuf::from("src/a.rs")));
        assert!(packet
            .pinned_paths
            .iter()
            .any(|p| p.ends_with("artifacts/design.md")));

        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn api_inlines_contents_pins_nothing() {
        let ws = temp_workspace("api");
        std::fs::create_dir_all(ws.join("src")).unwrap();
        std::fs::write(ws.join("src/a.rs"), "fn a() {}").unwrap();
        let bb = Blackboard::open(&ws).unwrap();
        bb.write_artifact("design.md", "the design").unwrap();
        let plan = plan_with_mention();

        let asm = ContextAssembler::new(&bb, &plan);
        let packet = asm.build(&plan.nodes[0], WorkerFamily::Api, None).unwrap();

        assert!(packet.pinned_paths.is_empty());
        let contents: Vec<&str> = packet.inlined_files.iter().map(|f| f.contents.as_str()).collect();
        assert!(contents.contains(&"fn a() {}"));
        assert!(contents.contains(&"the design"));

        let _ = std::fs::remove_dir_all(&ws);
    }
}

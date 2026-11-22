use rehearse::{ExecutionStep, Impact, Input, Operation, OperationMetadata, PlanBuilder};
use std::num::NonZeroUsize;

fn seed(name: &str, value: usize) -> Operation<(), usize, ()> {
    Operation::sync(
        OperationMetadata::new(name, Impact::Pure),
        (),
        move |_, ()| Ok(value),
    )
}

fn plus(name: &str, input: rehearse::Value<usize>) -> Operation<(), usize, ()> {
    Operation::sync(
        OperationMetadata::new(name, Impact::Read),
        Input::value(input),
        |_, value| Ok(value + 1),
    )
}

#[test]
fn graph_description_never_needs_context_clones_inputs_or_invokes_bodies() {
    enum NoContext {}
    struct Secret(&'static str);
    impl Clone for Secret {
        fn clone(&self) -> Self {
            panic!("literal must not be resolved: {}", self.0)
        }
    }
    #[derive(Clone)]
    struct NonSerializableOutput;

    let mut builder = PlanBuilder::<NoContext, ()>::new("static-only")
        .with_revision("application-build-42/config-7");
    let value = builder.add(
        Operation::sync(
            OperationMetadata::new("publish", Impact::Write),
            Input::literal(Secret("not-for-description")),
            |_: &NoContext, _: Secret| -> Result<NonSerializableOutput, ()> {
                panic!("description must never run a body")
            },
        )
        .with_annotation("environment", "production")
        .with_annotation("kind", "approval"),
    );
    let plan = builder.finish(value);
    let graph = plan.describe_graph();
    assert_eq!(graph.schema_version(), 1);
    assert_eq!(graph.plan_name(), "static-only");
    assert_eq!(graph.plan_revision(), Some("application-build-42/config-7"));
    assert_eq!(graph.default_interpretation(), "sequential");
    assert_eq!(graph.nodes().len(), 1);
    assert_eq!(graph.nodes()[0].annotations()["environment"], "production");
    assert_eq!(graph.nodes()[0].annotations()["kind"], "approval");
    assert_eq!(graph.nodes()[0].impact(), Impact::Write);
    assert!(graph.to_mermaid().contains("publish"));
    #[cfg(feature = "serde")]
    {
        let json = serde_json::to_value(&graph).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert!(json.get("dry_run_action").is_none());
        assert!(json["nodes"][0].get("dry_run_action").is_none());
        assert!(json["nodes"][0].get("value").is_none());
        assert!(!json.to_string().contains("not-for-description"));
    }
}

#[test]
fn a_value_fanout_keeps_sequential_control_order() {
    let mut builder = PlanBuilder::<(), ()>::new("sequential fanout");
    let input = builder.add(seed("source", 1));
    let left = builder.add(plus("left", input));
    let right = builder.add(plus("right", input));
    let output = builder.add(Operation::sync(
        OperationMetadata::new("join values", Impact::Pure),
        (Input::value(left), Input::value(right)),
        |_, (left, right)| Ok(left + right),
    ));
    let graph = builder.finish(output).describe_graph();
    assert_eq!(graph.plan_revision(), None);
    assert!(graph
        .execution()
        .iter()
        .all(|step| matches!(step, ExecutionStep::Node { .. })));
    assert_eq!(graph.nodes()[2].dependencies(), &[input.node()]);
    let mermaid = graph.to_mermaid();
    assert!(mermaid.contains("  n1 --> n2\n"));
    assert!(mermaid.contains("  n0 -.-> n2\n"));
    assert!(!mermaid.contains("_fork"));
    assert!(mermaid.contains("Default execute(): sequential"));
}

#[test]
fn explicit_parallel_regions_export_ordered_branches_and_join() {
    let mut builder = PlanBuilder::<(), ()>::new("release").with_revision("release-code/config-v2");
    let input = builder.add(seed("tested candidate", 1));
    let (dev, staging) =
        builder.parallel("non-production", NonZeroUsize::new(2).unwrap(), |group| {
            let dev = group.branch("dev", |branch| {
                let deployed =
                    branch.add(plus("deploy dev", input).with_annotation("environment", "dev"));
                branch.add(plus("verify dev", deployed))
            });
            let staging = group.branch("staging", |branch| {
                let deployed = branch
                    .add(plus("deploy staging", input).with_annotation("environment", "staging"));
                branch.add(plus("verify staging", deployed))
            });
            (dev, staging)
        });
    let approved = builder.add(
        Operation::sync(
            OperationMetadata::new("production approval", Impact::Session),
            (Input::value(dev), Input::value(staging)),
            |_, _| Ok(()),
        )
        .with_annotation("kind", "approval"),
    );
    let graph = builder.finish(approved).describe_graph();
    assert_eq!(
        graph
            .nodes()
            .iter()
            .map(|node| node.id().index())
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5]
    );
    assert_eq!(
        graph.nodes()[5].dependencies(),
        &[dev.node(), staging.node()]
    );
    let ExecutionStep::Parallel { group } = &graph.execution()[1] else {
        panic!("explicit region was lost")
    };
    assert_eq!(group.name, "non-production");
    assert_eq!(group.max_concurrency, 2);
    assert_eq!(
        group.branches[0].nodes,
        vec![graph.nodes()[1].id(), graph.nodes()[2].id()]
    );
    assert_eq!(
        group.branches[1].nodes,
        vec![graph.nodes()[3].id(), graph.nodes()[4].id()]
    );
    let mermaid = graph.to_mermaid();
    assert!(mermaid.contains("non-production\\nmax 2 concurrent"));
    assert!(mermaid.contains("_fork --> n1\n"));
    assert!(mermaid.contains("_fork --> n3\n"));
    assert!(mermaid.contains("  n1 --> n2\n"));
    assert!(mermaid.contains("  n3 --> n4\n"));
    assert!(!mermaid.contains("  n2 --> n3\n"));
    assert!(mermaid.contains("_join --> n5\n"));

    #[cfg(feature = "serde")]
    {
        let json = serde_json::to_value(&graph).unwrap();
        assert_eq!(json["default_interpretation"], "sequential");
        assert_eq!(json["execution"][0]["kind"], "node");
        assert_eq!(json["execution"][1]["kind"], "parallel");
        assert_eq!(json["execution"][1]["group"]["max_concurrency"], 2);
        assert_eq!(
            json["execution"][1]["group"]["branches"][0]["nodes"],
            serde_json::json!([1, 2])
        );
        assert_eq!(json["nodes"][1]["annotations"]["environment"], "dev");
        let decoded: rehearse::PlanGraphDescription = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, graph);
    }
}

#[test]
fn graph_annotations_are_metadata_and_labels_escape_mermaid_delimiters() {
    let mut builder = PlanBuilder::<(), ()>::new("plan\nnext line");
    let value = builder.add(
        seed("plain name", 7).with_annotation("label", "Display \"label\" <tag> \\ newline\nnext"),
    );
    let plan = builder.finish(value);
    let graph = plan.describe_graph();
    assert_eq!(graph.nodes()[0].name(), "plain name");
    assert_eq!(graph.nodes()[0].position(), 1);
    let mermaid = graph.to_mermaid();
    assert!(mermaid.contains("%% plan: plan next line\n"));
    assert!(mermaid.contains("Display &quot;label&quot; &lt;tag&gt; &#92; newline next"));
    assert!(!mermaid.contains("Display \"label\""));
}

#[cfg(feature = "serde")]
#[test]
fn adding_graph_export_preserves_existing_table_json_shape() {
    let mut builder = PlanBuilder::<(), ()>::new("legacy");
    let output = builder.add(seed("seed", 1));
    let plan = builder.finish(output);
    let old = serde_json::to_value(plan.describe()).unwrap();
    assert_eq!(old["plan_name"], "legacy");
    assert!(old.get("rows").is_some());
    assert!(old.get("nodes").is_none());
    assert!(old["rows"][0].get("dry_run_action").is_some());
    let graph = serde_json::to_value(plan.describe_graph()).unwrap();
    assert_eq!(graph["schema_version"], 1);
    assert!(graph.get("nodes").is_some());
    assert!(graph.get("rows").is_none());
}

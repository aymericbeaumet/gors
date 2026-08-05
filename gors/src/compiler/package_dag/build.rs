use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::compiler::ids::PackageId;

use super::model::{
    PackageCycle, PackageCycleHop, PackageDag, PackageDagEdge, PackageDagError, PackageDagImport,
    PackageDagLayer, PackageDagNode,
};

/// Validate and build one canonical dependency graph.
///
/// Inputs may arrive in any order. The result and every selected diagnostic are
/// independent of caller iteration order and scheduler width.
pub fn build_package_dag(
    entry: PackageId,
    nodes: impl IntoIterator<Item = PackageDagNode>,
    imports: impl IntoIterator<Item = PackageDagImport>,
) -> Result<PackageDag, PackageDagError> {
    let nodes = canonicalize_nodes(nodes)?;
    if nodes
        .binary_search_by_key(&entry, PackageDagNode::package)
        .is_err()
    {
        return Err(PackageDagError::UnknownEntry { entry });
    }
    let imports = canonicalize_imports(&nodes, imports)?;
    let edges = group_edges(&imports);
    let adjacency = adjacency(&nodes, &edges);
    let components = strongly_connected_components(&nodes, &adjacency);
    let cycles = build_cycles(&components, &adjacency, &imports)?;
    if !cycles.is_empty() {
        return Err(PackageDagError::Cycles {
            cycles: cycles.into(),
        });
    }
    let (layers, topological_order) = topological_layers(&nodes, &adjacency)?;
    Ok(PackageDag::new(
        entry,
        nodes.into(),
        edges.into(),
        layers.into(),
        topological_order.into(),
    ))
}

fn canonicalize_nodes(
    nodes: impl IntoIterator<Item = PackageDagNode>,
) -> Result<Vec<PackageDagNode>, PackageDagError> {
    let mut nodes = nodes.into_iter().collect::<Vec<_>>();
    nodes.sort();
    let mut canonical = Vec::<PackageDagNode>::with_capacity(nodes.len());
    for node in nodes {
        if let Some(previous) = canonical.last()
            && previous.package() == node.package()
        {
            if previous.key() != node.key() {
                return Err(PackageDagError::ConflictingPackageIdentity {
                    package: node.package(),
                    first: previous.key().clone(),
                    second: node.key().clone(),
                });
            }
            continue;
        }
        canonical.push(node);
    }

    let mut by_path = canonical
        .iter()
        .filter(|node| node.import_path().is_some())
        .collect::<Vec<_>>();
    by_path.sort_by(|left, right| {
        left.import_path()
            .cmp(&right.import_path())
            .then_with(|| left.package().cmp(&right.package()))
    });
    if let Some((first, second)) = by_path.windows(2).find_map(|pair| {
        let [first, second] = pair else {
            return None;
        };
        (first.import_path() == second.import_path() && first.package() != second.package())
            .then_some((*first, *second))
    }) {
        let Some(import_path) = first.import_path() else {
            return Err(PackageDagError::InconsistentTopology);
        };
        return Err(PackageDagError::ConflictingImportPath {
            import_path: import_path.clone(),
            first: first.package(),
            second: second.package(),
        });
    }
    Ok(canonical)
}

fn canonicalize_imports(
    nodes: &[PackageDagNode],
    imports: impl IntoIterator<Item = PackageDagImport>,
) -> Result<Vec<PackageDagImport>, PackageDagError> {
    let node_map = nodes
        .iter()
        .map(|node| (node.package(), node))
        .collect::<BTreeMap<_, _>>();
    let mut imports = imports.into_iter().collect::<Vec<_>>();
    imports.sort();
    imports.dedup();
    for import in &imports {
        if !node_map.contains_key(&import.importer()) {
            return Err(PackageDagError::UnknownImporter {
                importer: import.importer(),
                source: import.occurrence().source(),
            });
        }
        let Some(dependency) = node_map.get(&import.dependency()) else {
            return Err(PackageDagError::UnknownDependency {
                dependency: import.dependency(),
                source: import.occurrence().source(),
            });
        };
        let Some(expected_path) = dependency.import_path() else {
            return Err(PackageDagError::CommandLineDependency {
                dependency: dependency.package(),
                source: import.occurrence().source(),
            });
        };
        if expected_path != import.occurrence().canonical_path() {
            return Err(PackageDagError::DependencyPathMismatch {
                dependency: dependency.package(),
                expected: expected_path.clone(),
                found: import.occurrence().canonical_path().clone(),
                source: import.occurrence().source(),
            });
        }
    }
    Ok(imports)
}

fn group_edges(imports: &[PackageDagImport]) -> Vec<PackageDagEdge> {
    let mut grouped = BTreeMap::<(PackageId, PackageId), Vec<_>>::new();
    for import in imports {
        grouped
            .entry((import.importer(), import.dependency()))
            .or_default()
            .push(import.occurrence().clone());
    }
    grouped
        .into_iter()
        .map(|((importer, dependency), occurrences)| {
            PackageDagEdge::new(importer, dependency, occurrences.into())
        })
        .collect()
}

fn adjacency(
    nodes: &[PackageDagNode],
    edges: &[PackageDagEdge],
) -> BTreeMap<PackageId, Vec<PackageId>> {
    let mut adjacency = nodes
        .iter()
        .map(|node| (node.package(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for edge in edges {
        adjacency
            .entry(edge.importer())
            .or_default()
            .push(edge.dependency());
    }
    for dependencies in adjacency.values_mut() {
        dependencies.sort();
        dependencies.dedup();
    }
    adjacency
}

fn strongly_connected_components(
    nodes: &[PackageDagNode],
    adjacency: &BTreeMap<PackageId, Vec<PackageId>>,
) -> Vec<Vec<PackageId>> {
    let mut visited = BTreeSet::new();
    let mut finished = Vec::with_capacity(nodes.len());
    for node in nodes {
        let start = node.package();
        if visited.contains(&start) {
            continue;
        }
        let mut stack = vec![(start, false)];
        while let Some((package, expanded)) = stack.pop() {
            if expanded {
                finished.push(package);
                continue;
            }
            if !visited.insert(package) {
                continue;
            }
            stack.push((package, true));
            if let Some(dependencies) = adjacency.get(&package) {
                for dependency in dependencies.iter().rev() {
                    if !visited.contains(dependency) {
                        stack.push((*dependency, false));
                    }
                }
            }
        }
    }

    let mut transpose = nodes
        .iter()
        .map(|node| (node.package(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for (importer, dependencies) in adjacency {
        for dependency in dependencies {
            transpose.entry(*dependency).or_default().push(*importer);
        }
    }
    for importers in transpose.values_mut() {
        importers.sort();
        importers.dedup();
    }

    let mut assigned = BTreeSet::new();
    let mut components = Vec::new();
    for start in finished.into_iter().rev() {
        if !assigned.insert(start) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        while let Some(package) = stack.pop() {
            component.push(package);
            if let Some(importers) = transpose.get(&package) {
                for importer in importers.iter().rev() {
                    if assigned.insert(*importer) {
                        stack.push(*importer);
                    }
                }
            }
        }
        component.sort();
        components.push(component);
    }
    components.sort();
    components
}

fn build_cycles(
    components: &[Vec<PackageId>],
    adjacency: &BTreeMap<PackageId, Vec<PackageId>>,
    imports: &[PackageDagImport],
) -> Result<Vec<PackageCycle>, PackageDagError> {
    let mut imports_by_edge = BTreeMap::new();
    for import in imports {
        imports_by_edge
            .entry((import.importer(), import.dependency()))
            .or_insert(import);
    }
    let mut cycles = Vec::new();
    for component in components {
        let Some(start) = component.first().copied() else {
            continue;
        };
        let cyclic = component.len() > 1
            || adjacency
                .get(&start)
                .is_some_and(|dependencies| dependencies.binary_search(&start).is_ok());
        if !cyclic {
            continue;
        }
        let Some(path) = canonical_cycle_path(start, component, adjacency) else {
            return Err(PackageDagError::InconsistentTopology);
        };
        let mut hops = Vec::with_capacity(path.len().saturating_sub(1));
        for pair in path.windows(2) {
            let [importer, dependency] = pair else {
                return Err(PackageDagError::InconsistentTopology);
            };
            let Some(import) = imports_by_edge.get(&(*importer, *dependency)) else {
                return Err(PackageDagError::InconsistentTopology);
            };
            hops.push(PackageCycleHop::from_import(import));
        }
        cycles.push(PackageCycle::new(hops.into()));
    }
    cycles.sort();
    Ok(cycles)
}

fn canonical_cycle_path(
    start: PackageId,
    component: &[PackageId],
    adjacency: &BTreeMap<PackageId, Vec<PackageId>>,
) -> Option<Vec<PackageId>> {
    let members = component.iter().copied().collect::<BTreeSet<_>>();
    let dependencies = adjacency.get(&start)?;
    for dependency in dependencies
        .iter()
        .copied()
        .filter(|dependency| members.contains(dependency))
    {
        if dependency == start {
            return Some(vec![start, start]);
        }
        if let Some(mut suffix) = shortest_path(dependency, start, &members, adjacency) {
            let mut path = Vec::with_capacity(suffix.len().saturating_add(1));
            path.push(start);
            path.append(&mut suffix);
            return Some(path);
        }
    }
    None
}

fn shortest_path(
    from: PackageId,
    to: PackageId,
    members: &BTreeSet<PackageId>,
    adjacency: &BTreeMap<PackageId, Vec<PackageId>>,
) -> Option<Vec<PackageId>> {
    let mut queue = VecDeque::from([from]);
    let mut visited = BTreeSet::from([from]);
    let mut parent = BTreeMap::<PackageId, PackageId>::new();
    while let Some(package) = queue.pop_front() {
        for dependency in adjacency.get(&package).into_iter().flatten() {
            if !members.contains(dependency) || visited.contains(dependency) {
                continue;
            }
            visited.insert(*dependency);
            parent.insert(*dependency, package);
            if *dependency == to {
                let mut reversed = vec![to];
                let mut cursor = to;
                while cursor != from {
                    cursor = *parent.get(&cursor)?;
                    reversed.push(cursor);
                }
                reversed.reverse();
                return Some(reversed);
            }
            queue.push_back(*dependency);
        }
    }
    None
}

fn topological_layers(
    nodes: &[PackageDagNode],
    adjacency: &BTreeMap<PackageId, Vec<PackageId>>,
) -> Result<(Vec<PackageDagLayer>, Vec<PackageId>), PackageDagError> {
    let mut remaining = adjacency
        .iter()
        .map(|(package, dependencies)| (*package, dependencies.len()))
        .collect::<BTreeMap<_, _>>();
    let mut reverse = nodes
        .iter()
        .map(|node| (node.package(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for (importer, dependencies) in adjacency {
        for dependency in dependencies {
            reverse.entry(*dependency).or_default().push(*importer);
        }
    }
    for importers in reverse.values_mut() {
        importers.sort();
        importers.dedup();
    }

    let mut ready = remaining
        .iter()
        .filter_map(|(package, count)| (*count == 0).then_some(*package))
        .collect::<BTreeSet<_>>();
    let mut layers = Vec::new();
    let mut topological = Vec::with_capacity(nodes.len());
    while !ready.is_empty() {
        let layer = ready.iter().copied().collect::<Vec<_>>();
        ready.clear();
        topological.extend(layer.iter().copied());
        let mut next = BTreeSet::new();
        for dependency in &layer {
            for importer in reverse.get(dependency).into_iter().flatten() {
                let count = remaining
                    .get_mut(importer)
                    .ok_or(PackageDagError::InconsistentTopology)?;
                *count = count
                    .checked_sub(1)
                    .ok_or(PackageDagError::InconsistentTopology)?;
                if *count == 0 {
                    next.insert(*importer);
                }
            }
        }
        layers.push(PackageDagLayer::new(layer.into()));
        ready = next;
    }
    if topological.len() != nodes.len() {
        return Err(PackageDagError::InconsistentTopology);
    }
    Ok((layers, topological))
}

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt::Write;
use std::path::Path;

const UNREACHABLE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Edge {
    from: u32,
    to: u32,
    weight: u32,
}

struct Graph {
    offsets: Vec<usize>,
    edges: Vec<Edge>,
}

impl Graph {
    fn from_edges(vertices: u32, mut edges: Vec<Edge>) -> Result<Self, String> {
        if vertices == 0 {
            return Err("graph needs at least one vertex".into());
        }
        if edges.len() > u32::MAX as usize {
            return Err("graph edge count exceeds packed u32 IDs".into());
        }
        if edges
            .iter()
            .any(|edge| edge.from >= vertices || edge.to >= vertices)
        {
            return Err("graph endpoint outside vertex range".into());
        }
        edges.sort_unstable();
        let mut offsets = vec![0; vertices as usize + 1];
        for edge in &edges {
            offsets[edge.from as usize + 1] += 1;
        }
        for vertex in 1..offsets.len() {
            offsets[vertex] += offsets[vertex - 1];
        }
        Ok(Self { offsets, edges })
    }

    fn from_dimacs(input: &str) -> Result<Self, String> {
        let mut header = None;
        let mut edges = Vec::new();
        for (index, line) in input.lines().enumerate() {
            let line_number = index + 1;
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.is_empty() || fields[0] == "c" {
                continue;
            }
            let fail = |message: &str| format!("line {line_number}: {message}");
            if fields.len() != 4 {
                return Err(fail("expected four fields"));
            }
            let number = |text: &str| {
                if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(fail("expected unsigned 32-bit integer"));
                }
                text.parse::<u32>()
                    .map_err(|_| fail("expected unsigned 32-bit integer"))
            };
            match fields[0] {
                "p" => {
                    if header.is_some() {
                        return Err(fail("duplicate problem header"));
                    }
                    if fields[1] != "sp" {
                        return Err(fail("expected 'p sp' header"));
                    }
                    let vertices = number(fields[2])?;
                    if vertices == 0 {
                        return Err(fail("graph needs at least one vertex"));
                    }
                    header = Some((vertices, number(fields[3])?));
                }
                "a" => {
                    let (vertices, declared_edges) =
                        header.ok_or_else(|| fail("arc before problem header"))?;
                    let from = number(fields[1])?;
                    let to = number(fields[2])?;
                    let weight = number(fields[3])?;
                    if from == 0 || from > vertices || to == 0 || to > vertices {
                        return Err(fail("arc endpoint outside 1..vertices"));
                    }
                    if edges.len() >= declared_edges as usize {
                        return Err(fail("more arcs than declared"));
                    }
                    edges.push(Edge {
                        from: from - 1,
                        to: to - 1,
                        weight,
                    });
                }
                _ => return Err(fail("unknown record")),
            }
        }
        let (vertices, declared_edges) = header.ok_or("missing problem header")?;
        if edges.len() != declared_edges as usize {
            return Err("arc count differs from problem header".into());
        }
        Self::from_edges(vertices, edges)
    }

    fn undirected(&self) -> Result<Self, String> {
        let mut edges = Vec::new();
        for edge in &self.edges {
            edges.push(*edge);
            edges.push(Edge {
                from: edge.to,
                to: edge.from,
                weight: edge.weight,
            });
        }
        edges.sort_unstable();
        edges.dedup_by(|a, b| a.from == b.from && a.to == b.to);
        Self::from_edges(self.vertices() as u32, edges)
    }

    fn vertices(&self) -> usize {
        self.offsets.len() - 1
    }

    fn outgoing(&self, vertex: u32) -> impl Iterator<Item = (u32, Edge)> + '_ {
        let vertex = vertex as usize;
        (self.offsets[vertex]..self.offsets[vertex + 1]).map(|id| (id as u32, self.edges[id]))
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
struct Counts {
    pushes: u64,
    pops: u64,
}

#[derive(Debug, Eq, PartialEq)]
struct Distances {
    values: Vec<u32>,
    counts: Counts,
}

#[derive(Debug, Eq, PartialEq)]
struct Tree {
    parents: Vec<u32>,
    edge_ids: Vec<u32>,
    weight: u64,
    counts: Counts,
}

fn pack(key: u32, id: u32) -> u64 {
    (u64::from(key) << 32) | u64::from(id)
}

fn dijkstra(graph: &Graph) -> Result<Distances, String> {
    let mut result = Distances {
        values: vec![UNREACHABLE; graph.vertices()],
        counts: Counts { pushes: 1, pops: 0 },
    };
    let mut queue = BinaryHeap::new();
    result.values[0] = 0;
    queue.push(Reverse(pack(0, 0)));
    while let Some(Reverse(entry)) = queue.pop() {
        result.counts.pops += 1;
        let vertex = entry as u32;
        let distance = (entry >> 32) as u32;
        if distance != result.values[vertex as usize] {
            continue;
        }
        for (_, edge) in graph.outgoing(vertex) {
            let candidate = u64::from(distance) + u64::from(edge.weight);
            if candidate >= u64::from(UNREACHABLE) {
                return Err("Dijkstra candidate distance reaches reserved UINT32_MAX".into());
            }
            if candidate < u64::from(result.values[edge.to as usize]) {
                result.values[edge.to as usize] = candidate as u32;
                queue.push(Reverse(pack(candidate as u32, edge.to)));
                result.counts.pushes += 1;
            }
        }
    }
    Ok(result)
}

fn prim(graph: &Graph) -> Tree {
    let mut result = Tree {
        parents: vec![UNREACHABLE; graph.vertices()],
        edge_ids: Vec::with_capacity(graph.vertices() - 1),
        weight: 0,
        counts: Counts::default(),
    };
    let mut queue = BinaryHeap::new();
    let mut visited = vec![false; graph.vertices()];
    let relax =
        |vertex, visited: &[bool], queue: &mut BinaryHeap<Reverse<u64>>, counts: &mut Counts| {
            for (id, edge) in graph.outgoing(vertex) {
                if !visited[edge.to as usize] {
                    queue.push(Reverse(pack(edge.weight, id)));
                    counts.pushes += 1;
                }
            }
        };
    relax(0, &visited, &mut queue, &mut result.counts);
    visited[0] = true;
    result.parents[0] = 0;
    while let Some(Reverse(entry)) = queue.pop() {
        result.counts.pops += 1;
        let id = entry as u32;
        let edge = graph.edges[id as usize];
        if visited[edge.to as usize] {
            continue;
        }
        result.edge_ids.push(id);
        result.weight += u64::from(edge.weight);
        result.parents[edge.to as usize] = edge.from;
        visited[edge.to as usize] = true;
        relax(edge.to, &visited, &mut queue, &mut result.counts);
    }
    result
}

fn values(output: &mut String, name: &str, values: &[u32]) {
    output.push_str(name);
    for value in values {
        write!(output, ",{value}").unwrap();
    }
    output.push('\n');
}

fn verify(input: &str, workload: Option<&str>) -> Result<String, String> {
    if workload.is_some_and(|name| name != "Dijkstra" && name != "Prim") {
        return Err("--workload expects Dijkstra or Prim".into());
    }
    let graph = Graph::from_dimacs(input)?;
    let mut output = String::new();
    if workload.is_none() || workload == Some("Dijkstra") {
        let result = dijkstra(&graph)?;
        writeln!(
            output,
            "counts,Dijkstra,{},{}",
            result.counts.pushes, result.counts.pops
        )
        .unwrap();
        values(&mut output, "distances", &result.values);
    }
    if workload.is_none() || workload == Some("Prim") {
        let result = prim(&graph.undirected()?);
        writeln!(
            output,
            "counts,Prim,{},{}",
            result.counts.pushes, result.counts.pops
        )
        .unwrap();
        values(&mut output, "parents", &result.parents);
        writeln!(output, "weight,{}", result.weight).unwrap();
        values(&mut output, "tree_edges", &result.edge_ids);
    }
    Ok(output)
}

pub fn verify_file(path: &Path, workload: Option<&str>) -> Result<String, String> {
    let input =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    verify(&input, workload).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_weight_arcs_allow_decreasing_packed_ids() {
        let input = "p sp 3 2\na 1 3 0\na 3 2 0\n";
        assert_eq!(
            verify(input, None).unwrap(),
            "counts,Dijkstra,3,3\ndistances,0,0,0\ncounts,Prim,2,2\nparents,0,2,0\nweight,0\ntree_edges,0,3\n"
        );
    }

    #[test]
    fn parallel_asymmetric_edges_use_minimum_undirected_weight() {
        let input = "p sp 3 4\na 2 1 8\na 1 2 9\na 1 2 2\na 3 3 0\n";
        let graph = Graph::from_dimacs(input).unwrap();
        assert_eq!(graph.edges.len(), 4);
        assert_eq!(
            graph.undirected().unwrap().edges,
            vec![
                Edge {
                    from: 0,
                    to: 1,
                    weight: 2
                },
                Edge {
                    from: 1,
                    to: 0,
                    weight: 2
                },
                Edge {
                    from: 2,
                    to: 2,
                    weight: 0
                },
            ]
        );
        assert_eq!(
            verify(input, None).unwrap(),
            "counts,Dijkstra,2,2\ndistances,0,2,4294967295\ncounts,Prim,1,1\nparents,0,0,4294967295\nweight,2\ntree_edges,0\n"
        );
    }

    #[test]
    fn stale_entries_drain_and_disconnected_components_stay_unreachable() {
        let input = "p sp 6 7\na 1 2 7\na 1 2 7\na 1 3 1\na 1 4 1\na 3 2 0\na 4 2 0\na 5 6 0\n";
        assert_eq!(
            verify(input, None).unwrap(),
            "counts,Dijkstra,5,5\ndistances,0,1,1,1,4294967295,4294967295\ncounts,Prim,5,5\nparents,0,2,0,1,4294967295,4294967295\nweight,1\ntree_edges,1,7,5\n"
        );
    }

    #[test]
    fn singleton_and_source_self_loop_keep_defined_counts() {
        assert_eq!(
            verify("p sp 1 0\n", None).unwrap(),
            "counts,Dijkstra,1,1\ndistances,0\ncounts,Prim,0,0\nparents,0\nweight,0\ntree_edges\n"
        );
        assert_eq!(
            verify("p sp 1 1\na 1 1 0\n", Some("Prim")).unwrap(),
            "counts,Prim,1,1\nparents,0\nweight,0\ntree_edges\n"
        );
    }

    #[test]
    fn dijkstra_rejects_reserved_distance_even_when_edge_cannot_improve() {
        for input in [
            "p sp 2 1\na 1 2 4294967295\n",
            "p sp 3 2\na 1 2 4294967294\na 2 3 2\n",
            "p sp 2 2\na 1 2 1\na 2 1 4294967294\n",
        ] {
            assert!(
                verify(input, Some("Dijkstra"))
                    .unwrap_err()
                    .contains("reserved UINT32_MAX")
            );
        }
        assert_eq!(
            verify("p sp 2 1\na 1 2 4294967294\n", Some("Dijkstra")).unwrap(),
            "counts,Dijkstra,2,2\ndistances,0,4294967294\n"
        );
        assert_eq!(
            verify("p sp 3 1\na 2 3 4294967295\n", Some("Dijkstra")).unwrap(),
            "counts,Dijkstra,1,1\ndistances,0,4294967295,4294967295\n"
        );
    }

    #[test]
    fn prim_sums_full_u32_weights_in_u64() {
        assert_eq!(
            verify(
                "p sp 3 2\na 1 2 4294967295\na 2 3 4294967295\n",
                Some("Prim")
            )
            .unwrap(),
            "counts,Prim,2,2\nparents,0,0,1\nweight,8589934590\ntree_edges,0,2\n"
        );
    }

    #[test]
    fn dimacs_accepts_whitespace_and_rejects_invalid_graphs() {
        assert!(verify("\n  c fixture\r\n\tp\tsp 2 1\n a 1 2 0\n", None).is_ok());
        for input in [
            "",
            "p sp 0 0\n",
            "p sp 2 1\n",
            "a 1 2 3\n",
            "p sp 2 0\np sp 2 0\n",
            "p xx 2 0\n",
            "p sp 2 1\na 0 2 3\n",
            "p sp 2 1\na 1 3 3\n",
            "p sp 2 1\na 1 2 -1\n",
            "p sp 2 1\na 1 2 +1\n",
            "p sp 2 1\na 1 2 4294967296\n",
            "p sp 4294967296 0\n",
            "p sp 2 4294967296\n",
            "p sp 2 0\na 1 2 0\n",
            "p sp 2 1\na 1 2\n",
            "p sp 2 1\na 1 2 0 extra\n",
            "p sp 2 0\nx 1 2 3\n",
        ] {
            assert!(verify(input, None).is_err(), "accepted {input:?}");
        }
    }
}

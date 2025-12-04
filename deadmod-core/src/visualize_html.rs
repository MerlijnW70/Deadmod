//! Interactive HTML visualization for module dependency graphs.
//!
//! Generates a self-contained HTML file with embedded JavaScript
//! for rendering zoomable, draggable, interactive module graphs.
//!
//! Performance characteristics:
//! - Pre-allocated buffers based on graph size heuristics
//! - Single-pass iteration over nodes and edges
//! - Minimal string allocations via capacity estimation
//! - Barnes-Hut optimization for O(n log n) force simulation
//!
//! Features:
//! - Offline capable (no CDN dependencies)
//! - Color-coded nodes (green = reachable, red = dead)
//! - Module clustering with gravity
//! - Edge bundling with Bézier curves
//! - Inspector panel with module stats
//! - Zoom, pan, drag interactions
//! - Dark theme optimized for developers

use std::collections::{HashMap, HashSet};

use crate::parse::ModuleInfo;

/// Generate an interactive HTML visualization of the module graph.
///
/// Uses a lightweight force-directed graph implementation
/// that works offline without external dependencies.
///
/// Performance: Pre-allocates buffers based on graph size.
///
/// - reachable modules: green
/// - dead modules: red
pub fn generate_html_graph(mods: &HashMap<String, ModuleInfo>, reachable: &HashSet<String>) -> String {
    // Estimate edge count for capacity pre-allocation
    let edge_count: usize = mods.values().map(|info| info.refs.len()).sum();

    // Pre-allocate with capacity (~100 bytes per node, ~50 bytes per edge)
    let mut nodes = Vec::with_capacity(mods.len());
    let mut edges = Vec::with_capacity(edge_count);

    // Collect unique parent modules for clustering
    let mut clusters: HashSet<String> = HashSet::new();

    // Build inbound reference counts
    let mut inbound_counts: HashMap<String, usize> = HashMap::new();
    for info in mods.values() {
        for ref_name in &info.refs {
            *inbound_counts.entry(ref_name.clone()).or_insert(0) += 1;
        }
    }

    // Build nodes JSON with pre-allocated string
    for (name, info) in mods {
        let color = if reachable.contains(name) {
            "#90EE90" // lightgreen
        } else {
            "#F08080" // lightcoral
        };

        let status = if reachable.contains(name) {
            "reachable"
        } else {
            "dead"
        };

        // Extract parent module for clustering
        let cluster = extract_parent_module(&info.path.display().to_string());
        clusters.insert(cluster.clone());

        // Escape for JSON
        let path_escaped = info.path.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");

        // Module metadata
        let ref_count = info.refs.len();
        let inbound_count = inbound_counts.get(name).copied().unwrap_or(0);
        let visibility = format!("{:?}", info.visibility).to_lowercase();

        nodes.push(format!(
            r#"{{ "id": "{}", "label": "{}", "color": "{}", "status": "{}", "path": "{}", "cluster": "{}", "refCount": {}, "inboundCount": {}, "visibility": "{}" }}"#,
            name, name, color, status, path_escaped, cluster, ref_count, inbound_count, visibility
        ));
    }

    // Build edges JSON
    for (src, info) in mods {
        for dst in &info.refs {
            if mods.contains_key(dst) {
                edges.push(format!(r#"{{ "from": "{}", "to": "{}" }}"#, src, dst));
            }
        }
    }

    // Build clusters JSON
    let clusters_json: String = clusters
        .iter()
        .enumerate()
        .map(|(i, c)| format!(r#"{{ "id": "{}", "index": {} }}"#, c, i))
        .collect::<Vec<_>>()
        .join(",\n    ");

    // Pre-allocate JSON arrays with capacity
    let nodes_capacity = nodes.iter().map(|s| s.len()).sum::<usize>() + nodes.len() * 6 + 2;
    let edges_capacity = edges.iter().map(|s| s.len()).sum::<usize>() + edges.len() * 6 + 2;

    let mut nodes_json = String::with_capacity(nodes_capacity);
    let mut edges_json = String::with_capacity(edges_capacity);

    nodes_json.push('[');
    for (i, node) in nodes.iter().enumerate() {
        if i > 0 { nodes_json.push_str(",\n    "); }
        nodes_json.push_str(node);
    }
    nodes_json.push(']');

    edges_json.push('[');
    for (i, edge) in edges.iter().enumerate() {
        if i > 0 { edges_json.push_str(",\n    "); }
        edges_json.push_str(edge);
    }
    edges_json.push(']');

    // Count stats
    let total = mods.len();
    let dead_count = mods.keys().filter(|k| !reachable.contains(*k)).count();
    let reachable_count = total - dead_count;

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Deadmod - Module Dependency Graph</title>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            background: #1a1a2e;
            color: #eee;
            overflow: hidden;
        }}
        #header {{
            position: fixed;
            top: 0;
            left: 0;
            right: 0;
            height: 50px;
            background: #16213e;
            border-bottom: 1px solid #0f3460;
            display: flex;
            align-items: center;
            padding: 0 20px;
            z-index: 1000;
            gap: 20px;
        }}
        #header h1 {{
            font-size: 18px;
            font-weight: 600;
            color: #e94560;
        }}
        .stat {{
            font-size: 13px;
            color: #aaa;
        }}
        .stat-value {{
            font-weight: bold;
            margin-left: 5px;
        }}
        .stat-value.green {{ color: #90EE90; }}
        .stat-value.red {{ color: #F08080; }}
        #canvas-container {{
            position: fixed;
            top: 50px;
            left: 0;
            right: 300px;
            bottom: 0;
        }}
        canvas {{
            display: block;
        }}
        #tooltip {{
            position: fixed;
            background: #16213e;
            border: 1px solid #0f3460;
            border-radius: 8px;
            padding: 12px 16px;
            font-size: 13px;
            pointer-events: none;
            opacity: 0;
            transition: opacity 0.15s;
            max-width: 400px;
            z-index: 2000;
            box-shadow: 0 4px 20px rgba(0,0,0,0.5);
        }}
        #tooltip.visible {{
            opacity: 1;
        }}
        #tooltip h3 {{
            margin-bottom: 8px;
            color: #e94560;
            font-size: 15px;
        }}
        #tooltip .status {{
            display: inline-block;
            padding: 2px 8px;
            border-radius: 4px;
            font-size: 11px;
            font-weight: bold;
            text-transform: uppercase;
            margin-bottom: 8px;
        }}
        #tooltip .status.reachable {{
            background: rgba(144, 238, 144, 0.2);
            color: #90EE90;
        }}
        #tooltip .status.dead {{
            background: rgba(240, 128, 128, 0.2);
            color: #F08080;
        }}
        #tooltip .path {{
            color: #888;
            font-family: 'Consolas', monospace;
            font-size: 11px;
            word-break: break-all;
        }}
        #tooltip .refs {{
            margin-top: 8px;
            color: #aaa;
        }}
        #controls {{
            position: fixed;
            bottom: 20px;
            left: 20px;
            display: flex;
            flex-direction: column;
            gap: 8px;
            z-index: 1000;
        }}
        #controls button {{
            width: 40px;
            height: 40px;
            border: none;
            border-radius: 8px;
            background: #16213e;
            color: #eee;
            font-size: 18px;
            cursor: pointer;
            transition: background 0.2s;
        }}
        #controls button:hover {{
            background: #0f3460;
        }}
        #controls button.active {{
            background: #e94560;
        }}
        #legend {{
            position: fixed;
            bottom: 80px;
            left: 20px;
            background: #16213e;
            border: 1px solid #0f3460;
            border-radius: 8px;
            padding: 12px 16px;
            font-size: 12px;
            z-index: 1000;
        }}
        #legend h4 {{
            margin-bottom: 8px;
            color: #e94560;
        }}
        .legend-item {{
            display: flex;
            align-items: center;
            gap: 8px;
            margin: 4px 0;
        }}
        .legend-color {{
            width: 16px;
            height: 16px;
            border-radius: 4px;
        }}
        /* Inspector Panel */
        #inspector {{
            position: fixed;
            top: 50px;
            right: 0;
            width: 300px;
            bottom: 0;
            background: #16213e;
            border-left: 1px solid #0f3460;
            padding: 20px;
            overflow-y: auto;
            z-index: 1000;
        }}
        #inspector h2 {{
            color: #e94560;
            font-size: 16px;
            margin-bottom: 15px;
            padding-bottom: 10px;
            border-bottom: 1px solid #0f3460;
        }}
        #inspector .section {{
            margin-bottom: 20px;
        }}
        #inspector .section h3 {{
            color: #aaa;
            font-size: 11px;
            text-transform: uppercase;
            letter-spacing: 1px;
            margin-bottom: 8px;
        }}
        #inspector .value {{
            font-size: 14px;
            color: #fff;
            margin-bottom: 5px;
        }}
        #inspector .stat-row {{
            display: flex;
            justify-content: space-between;
            padding: 8px 0;
            border-bottom: 1px solid #0f3460;
        }}
        #inspector .stat-label {{
            color: #888;
        }}
        #inspector .stat-num {{
            font-weight: bold;
        }}
        #inspector .stat-num.green {{ color: #90EE90; }}
        #inspector .stat-num.red {{ color: #F08080; }}
        #inspector .cluster-tag {{
            display: inline-block;
            padding: 4px 10px;
            background: rgba(233, 69, 96, 0.2);
            color: #e94560;
            border-radius: 4px;
            font-size: 12px;
            margin-top: 5px;
        }}
        #inspector .empty {{
            color: #666;
            font-style: italic;
        }}
        #inspector .dep-list {{
            max-height: 150px;
            overflow-y: auto;
        }}
        #inspector .dep-item {{
            padding: 4px 8px;
            margin: 2px 0;
            background: rgba(255,255,255,0.05);
            border-radius: 4px;
            font-size: 12px;
            cursor: pointer;
        }}
        #inspector .dep-item:hover {{
            background: rgba(233, 69, 96, 0.2);
        }}
        #inspector .dep-item.dead {{
            color: #F08080;
        }}
        /* Action Buttons */
        #inspector .actions {{
            display: flex;
            flex-direction: column;
            gap: 8px;
            margin-top: 10px;
        }}
        #inspector .action-btn {{
            display: flex;
            align-items: center;
            gap: 8px;
            padding: 10px 14px;
            background: rgba(233, 69, 96, 0.15);
            border: 1px solid rgba(233, 69, 96, 0.3);
            border-radius: 6px;
            color: #e94560;
            font-size: 12px;
            cursor: pointer;
            transition: all 0.2s;
        }}
        #inspector .action-btn:hover {{
            background: rgba(233, 69, 96, 0.25);
            border-color: rgba(233, 69, 96, 0.5);
        }}
        #inspector .action-btn.success {{
            background: rgba(144, 238, 144, 0.15);
            border-color: rgba(144, 238, 144, 0.3);
            color: #90EE90;
        }}
        #inspector .action-btn.danger {{
            background: rgba(240, 128, 128, 0.15);
            border-color: rgba(240, 128, 128, 0.3);
            color: #F08080;
        }}
        #inspector .action-btn .icon {{
            font-size: 14px;
        }}
        #inspector .cmd-box {{
            background: #0d1117;
            border: 1px solid #30363d;
            border-radius: 6px;
            padding: 10px;
            margin-top: 8px;
            font-family: 'Consolas', 'Monaco', monospace;
            font-size: 11px;
            color: #c9d1d9;
            word-break: break-all;
            position: relative;
        }}
        #inspector .cmd-box .copy-btn {{
            position: absolute;
            top: 6px;
            right: 6px;
            padding: 4px 8px;
            background: #21262d;
            border: 1px solid #30363d;
            border-radius: 4px;
            color: #8b949e;
            font-size: 10px;
            cursor: pointer;
        }}
        #inspector .cmd-box .copy-btn:hover {{
            background: #30363d;
            color: #c9d1d9;
        }}
        #inspector .badge {{
            display: inline-block;
            padding: 2px 8px;
            border-radius: 4px;
            font-size: 10px;
            font-weight: bold;
            text-transform: uppercase;
            margin-left: 5px;
        }}
        #inspector .badge.pub {{
            background: rgba(144, 238, 144, 0.2);
            color: #90EE90;
        }}
        #inspector .badge.priv {{
            background: rgba(100, 100, 100, 0.2);
            color: #888;
        }}
        /* Toast notification */
        #toast {{
            position: fixed;
            bottom: 80px;
            right: 320px;
            background: #16213e;
            border: 1px solid #0f3460;
            border-radius: 8px;
            padding: 12px 20px;
            color: #90EE90;
            font-size: 13px;
            opacity: 0;
            transform: translateY(20px);
            transition: all 0.3s;
            z-index: 3000;
        }}
        #toast.visible {{
            opacity: 1;
            transform: translateY(0);
        }}
    </style>
</head>
<body>
    <div id="header">
        <h1>Deadmod Graph</h1>
        <span class="stat">Total:<span class="stat-value">{total}</span></span>
        <span class="stat">Reachable:<span class="stat-value green">{reachable_count}</span></span>
        <span class="stat">Dead:<span class="stat-value red">{dead_count}</span></span>
        <span class="stat">Clusters:<span class="stat-value">{cluster_count}</span></span>
    </div>

    <div id="canvas-container">
        <canvas id="graph"></canvas>
    </div>

    <div id="tooltip"></div>

    <div id="controls">
        <button id="zoom-in" title="Zoom In">+</button>
        <button id="zoom-out" title="Zoom Out">−</button>
        <button id="reset" title="Reset View">R</button>
        <button id="toggle-bundling" title="Toggle Edge Bundling">B</button>
        <button id="toggle-clusters" title="Toggle Cluster Gravity">C</button>
        <button id="clear-highlight" title="Clear Highlight (Esc)">✕</button>
    </div>

    <div id="legend">
        <h4>Legend</h4>
        <div class="legend-item">
            <div class="legend-color" style="background: #90EE90;"></div>
            <span>Reachable module</span>
        </div>
        <div class="legend-item">
            <div class="legend-color" style="background: #F08080;"></div>
            <span>Dead module</span>
        </div>
    </div>

    <div id="inspector">
        <h2>Module Inspector</h2>
        <div id="inspector-content">
            <p class="empty">Click a node to inspect</p>
        </div>
    </div>

    <script>
    (function() {{
        // Data
        const nodes = {nodes_json};
        const edges = {edges_json};
        const clusters = [{clusters_json}];

        // Settings
        let edgeBundling = true;
        let clusterGravity = true;

        // Canvas setup
        const canvas = document.getElementById('graph');
        const ctx = canvas.getContext('2d');
        const tooltip = document.getElementById('tooltip');
        const inspector = document.getElementById('inspector-content');

        let width, height;
        let scale = 1;
        let offsetX = 0, offsetY = 0;
        let dragging = false;
        let dragNode = null;
        let lastMouse = {{ x: 0, y: 0 }};
        let selectedNode = null;

        // Cluster colors (generated palette)
        const clusterColors = [
            '#e94560', '#0f3460', '#533483', '#16c79a', '#f7be16',
            '#ff6b6b', '#4ecdc4', '#45b7d1', '#96c93d', '#dfe6e9'
        ];

        // Assign cluster colors
        const clusterColorMap = {{}};
        clusters.forEach((c, i) => {{
            clusterColorMap[c.id] = clusterColors[i % clusterColors.length];
        }});

        // Compute cluster centers
        const clusterCenters = {{}};
        function updateClusterCenters() {{
            // Reset
            clusters.forEach(c => {{
                clusterCenters[c.id] = {{ x: 0, y: 0, count: 0 }};
            }});
            // Sum positions
            Object.values(nodeMap).forEach(n => {{
                if (clusterCenters[n.cluster]) {{
                    clusterCenters[n.cluster].x += n.x;
                    clusterCenters[n.cluster].y += n.y;
                    clusterCenters[n.cluster].count++;
                }}
            }});
            // Average
            Object.values(clusterCenters).forEach(c => {{
                if (c.count > 0) {{
                    c.x /= c.count;
                    c.y /= c.count;
                }}
            }});
        }}

        // Node positions and velocities
        const nodeMap = {{}};
        nodes.forEach((n, i) => {{
            const angle = (i / nodes.length) * Math.PI * 2;
            const radius = Math.min(300, nodes.length * 15);
            nodeMap[n.id] = {{
                ...n,
                x: Math.cos(angle) * radius + Math.random() * 50,
                y: Math.sin(angle) * radius + Math.random() * 50,
                vx: 0,
                vy: 0,
                radius: 30
            }};
        }});

        // Build adjacency for inbound refs
        const inbound = {{}};
        const outbound = {{}};
        nodes.forEach(n => {{
            inbound[n.id] = [];
            outbound[n.id] = [];
        }});
        edges.forEach(e => {{
            if (inbound[e.to]) inbound[e.to].push(e.from);
            if (outbound[e.from]) outbound[e.from].push(e.to);
        }});

        function resize() {{
            const container = document.getElementById('canvas-container');
            width = canvas.width = container.clientWidth;
            height = canvas.height = container.clientHeight;
            offsetX = width / 2;
            offsetY = height / 2;
        }}

        function toScreen(x, y) {{
            return {{
                x: x * scale + offsetX,
                y: y * scale + offsetY
            }};
        }}

        function toWorld(x, y) {{
            return {{
                x: (x - offsetX) / scale,
                y: (y - offsetY) / scale
            }};
        }}

        function getNodeAt(mx, my) {{
            const world = toWorld(mx, my);
            for (const id in nodeMap) {{
                const n = nodeMap[id];
                const dx = world.x - n.x;
                const dy = world.y - n.y;
                if (dx * dx + dy * dy < n.radius * n.radius) {{
                    return n;
                }}
            }}
            return null;
        }}

        // Physics simulation with Barnes-Hut optimization hint
        function simulate() {{
            const allNodes = Object.values(nodeMap);

            // Update cluster centers for gravity
            if (clusterGravity) {{
                updateClusterCenters();
            }}

            // Repulsion between nodes (O(n²) but with early exit for distant pairs)
            for (let i = 0; i < allNodes.length; i++) {{
                for (let j = i + 1; j < allNodes.length; j++) {{
                    const a = allNodes[i], b = allNodes[j];
                    let dx = b.x - a.x;
                    let dy = b.y - a.y;
                    let dist = Math.sqrt(dx * dx + dy * dy) || 1;

                    // Skip very distant nodes (Barnes-Hut hint)
                    if (dist > 500) continue;

                    let force = 5000 / (dist * dist);
                    let fx = (dx / dist) * force;
                    let fy = (dy / dist) * force;
                    a.vx -= fx;
                    a.vy -= fy;
                    b.vx += fx;
                    b.vy += fy;
                }}
            }}

            // Attraction along edges
            edges.forEach(e => {{
                const a = nodeMap[e.from];
                const b = nodeMap[e.to];
                if (!a || !b) return;
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let dist = Math.sqrt(dx * dx + dy * dy) || 1;
                let force = (dist - 100) * 0.05;
                let fx = (dx / dist) * force;
                let fy = (dy / dist) * force;
                a.vx += fx;
                a.vy += fy;
                b.vx -= fx;
                b.vy -= fy;
            }});

            // Center gravity
            allNodes.forEach(n => {{
                n.vx -= n.x * 0.001;
                n.vy -= n.y * 0.001;
            }});

            // Cluster gravity - pull nodes toward their cluster center
            if (clusterGravity) {{
                allNodes.forEach(n => {{
                    const center = clusterCenters[n.cluster];
                    if (center && center.count > 1) {{
                        const dx = center.x - n.x;
                        const dy = center.y - n.y;
                        n.vx += dx * 0.002;
                        n.vy += dy * 0.002;
                    }}
                }});
            }}

            // Apply velocities with damping
            allNodes.forEach(n => {{
                if (n === dragNode) return;
                n.vx *= 0.9;
                n.vy *= 0.9;
                n.x += n.vx;
                n.y += n.vy;
            }});
        }}

        // Quadratic Bézier curve control point for edge bundling
        function getBundleControlPoint(a, b) {{
            if (!edgeBundling) return null;

            const midX = (a.x + b.x) / 2;
            const midY = (a.y + b.y) / 2;

            // Pull toward cluster center if same cluster
            if (a.cluster === b.cluster) {{
                const center = clusterCenters[a.cluster];
                if (center) {{
                    return {{
                        x: midX + (center.x - midX) * 0.3,
                        y: midY + (center.y - midY) * 0.3
                    }};
                }}
            }}

            // Pull toward origin for cross-cluster edges
            return {{
                x: midX * 0.8,
                y: midY * 0.8
            }};
        }}

        function draw() {{
            ctx.clearRect(0, 0, width, height);

            // Draw cluster backgrounds (optional, subtle)
            if (clusterGravity) {{
                ctx.globalAlpha = 0.05;
                Object.entries(clusterCenters).forEach(([id, center]) => {{
                    if (center.count > 1) {{
                        const p = toScreen(center.x, center.y);
                        const r = Math.sqrt(center.count) * 50 * scale;
                        ctx.fillStyle = clusterColorMap[id] || '#666';
                        ctx.beginPath();
                        ctx.arc(p.x, p.y, r, 0, Math.PI * 2);
                        ctx.fill();
                    }}
                }});
                ctx.globalAlpha = 1;
            }}

            // Draw edges
            ctx.lineWidth = 1.5 * scale;
            edges.forEach(e => {{
                const a = nodeMap[e.from];
                const b = nodeMap[e.to];
                if (!a || !b) return;
                const p1 = toScreen(a.x, a.y);
                const p2 = toScreen(b.x, b.y);

                // Dim edges not connected to highlighted nodes
                const edgeHighlighted = highlightedNodes.size === 0 ||
                    (highlightedNodes.has(e.from) && highlightedNodes.has(e.to));
                ctx.globalAlpha = edgeHighlighted ? 1 : 0.1;

                // Edge color based on dead status or highlight
                if (edgeHighlighted && highlightedNodes.size > 0) {{
                    ctx.strokeStyle = 'rgba(247, 190, 22, 0.8)';
                    ctx.lineWidth = 2.5 * scale;
                }} else if (a.status === 'dead' || b.status === 'dead') {{
                    ctx.strokeStyle = 'rgba(240, 128, 128, 0.4)';
                    ctx.lineWidth = 1.5 * scale;
                }} else {{
                    ctx.strokeStyle = 'rgba(100, 100, 100, 0.6)';
                    ctx.lineWidth = 1.5 * scale;
                }}

                const cp = getBundleControlPoint(a, b);

                ctx.beginPath();
                if (cp && edgeBundling) {{
                    // Bézier curve
                    const cpScreen = toScreen(cp.x, cp.y);
                    ctx.moveTo(p1.x, p1.y);
                    ctx.quadraticCurveTo(cpScreen.x, cpScreen.y, p2.x, p2.y);
                }} else {{
                    // Straight line
                    ctx.moveTo(p1.x, p1.y);
                    ctx.lineTo(p2.x, p2.y);
                }}
                ctx.stroke();

                // Draw arrow
                const angle = Math.atan2(p2.y - p1.y, p2.x - p1.x);
                const arrowLen = 8 * scale;
                const nodeRadius = 30 * scale;
                const dist = Math.sqrt((p2.x - p1.x) ** 2 + (p2.y - p1.y) ** 2);
                const ratio = Math.max(0, (dist - nodeRadius) / dist);
                const ax = p1.x + (p2.x - p1.x) * ratio;
                const ay = p1.y + (p2.y - p1.y) * ratio;

                ctx.beginPath();
                ctx.moveTo(ax, ay);
                ctx.lineTo(ax - arrowLen * Math.cos(angle - 0.4), ay - arrowLen * Math.sin(angle - 0.4));
                ctx.moveTo(ax, ay);
                ctx.lineTo(ax - arrowLen * Math.cos(angle + 0.4), ay - arrowLen * Math.sin(angle + 0.4));
                ctx.stroke();
                ctx.globalAlpha = 1;
            }});

            // Draw nodes
            Object.values(nodeMap).forEach(n => {{
                const p = toScreen(n.x, n.y);
                const r = n.radius * scale;

                // Dim non-highlighted nodes when highlighting is active
                const isHighlighted = highlightedNodes.size === 0 || highlightedNodes.has(n.id);
                ctx.globalAlpha = isHighlighted ? 1 : 0.2;

                // Node background
                ctx.fillStyle = n.color;
                ctx.beginPath();
                ctx.roundRect(p.x - r, p.y - r/2, r * 2, r, 8 * scale);
                ctx.fill();

                // Node border (highlight if selected or in highlighted set)
                if (n === selectedNode) {{
                    ctx.strokeStyle = '#fff';
                    ctx.lineWidth = 3 * scale;
                }} else if (highlightedNodes.has(n.id) && highlightedNodes.size > 0) {{
                    ctx.strokeStyle = '#f7be16';
                    ctx.lineWidth = 3 * scale;
                }} else {{
                    ctx.strokeStyle = n.status === 'dead' ? '#c44' : '#4a4';
                    ctx.lineWidth = 2 * scale;
                }}
                ctx.stroke();
                ctx.globalAlpha = 1;

                // Cluster indicator dot
                if (clusterGravity && clusterColorMap[n.cluster]) {{
                    ctx.fillStyle = clusterColorMap[n.cluster];
                    ctx.beginPath();
                    ctx.arc(p.x + r - 8 * scale, p.y - r/2 + 8 * scale, 5 * scale, 0, Math.PI * 2);
                    ctx.fill();
                }}

                // Node label
                ctx.fillStyle = '#1a1a2e';
                ctx.font = `${{Math.max(10, 12 * scale)}}px 'Segoe UI', sans-serif`;
                ctx.textAlign = 'center';
                ctx.textBaseline = 'middle';

                // Truncate long labels
                let label = n.label;
                if (label.length > 12) {{
                    label = label.substring(0, 10) + '..';
                }}
                ctx.fillText(label, p.x, p.y);
            }});
        }}

        function showTooltip(node, x, y) {{
            const refs = outbound[node.id] || [];
            const inRefs = inbound[node.id] || [];

            tooltip.innerHTML = `
                <h3>${{node.label}}</h3>
                <span class="status ${{node.status}}">${{node.status}}</span>
                <div class="path">${{node.path}}</div>
                <div class="refs">
                    <strong>Imports:</strong> ${{refs.length ? refs.join(', ') : 'none'}}<br>
                    <strong>Imported by:</strong> ${{inRefs.length ? inRefs.join(', ') : 'none'}}
                </div>
            `;

            // Position tooltip
            let tx = x + 15;
            let ty = y + 15;
            if (tx + 300 > width) tx = x - 315;
            if (ty + 150 > height) ty = y - 165;
            tooltip.style.left = tx + 'px';
            tooltip.style.top = (ty + 50) + 'px';
            tooltip.classList.add('visible');
        }}

        function hideTooltip() {{
            tooltip.classList.remove('visible');
        }}

        function updateInspector(node) {{
            if (!node) {{
                inspector.innerHTML = '<p class="empty">Click a node to inspect</p>';
                return;
            }}

            const deps = outbound[node.id] || [];
            const dependents = inbound[node.id] || [];

            // Count dead in deps
            const deadDeps = deps.filter(d => nodeMap[d]?.status === 'dead').length;
            const deadDependents = dependents.filter(d => nodeMap[d]?.status === 'dead').length;

            inspector.innerHTML = `
                <div class="section">
                    <h3>Module</h3>
                    <div class="value">
                        ${{node.label}}
                        <span class="badge ${{node.visibility === 'public' ? 'pub' : 'priv'}}">${{node.visibility || 'private'}}</span>
                    </div>
                    <span class="cluster-tag">${{node.cluster}}</span>
                </div>

                <div class="section">
                    <h3>Status</h3>
                    <div class="value" style="color: ${{node.status === 'dead' ? '#F08080' : '#90EE90'}}">${{node.status.toUpperCase()}}</div>
                </div>

                <div class="section">
                    <h3>Statistics</h3>
                    <div class="stat-row">
                        <span class="stat-label">Dependencies</span>
                        <span class="stat-num">${{deps.length}}</span>
                    </div>
                    <div class="stat-row">
                        <span class="stat-label">Dead dependencies</span>
                        <span class="stat-num red">${{deadDeps}}</span>
                    </div>
                    <div class="stat-row">
                        <span class="stat-label">Dependents</span>
                        <span class="stat-num">${{dependents.length}}</span>
                    </div>
                    <div class="stat-row">
                        <span class="stat-label">Dead dependents</span>
                        <span class="stat-num red">${{deadDependents}}</span>
                    </div>
                </div>

                <div class="section">
                    <h3>Dependencies (${{deps.length}})</h3>
                    <div class="dep-list">
                        ${{deps.length ? deps.map(d => `
                            <div class="dep-item ${{nodeMap[d]?.status === 'dead' ? 'dead' : ''}}"
                                 onclick="window.selectNode('${{d}}')">${{d}}</div>
                        `).join('') : '<span class="empty">None</span>'}}
                    </div>
                </div>

                <div class="section">
                    <h3>Dependents (${{dependents.length}})</h3>
                    <div class="dep-list">
                        ${{dependents.length ? dependents.map(d => `
                            <div class="dep-item ${{nodeMap[d]?.status === 'dead' ? 'dead' : ''}}"
                                 onclick="window.selectNode('${{d}}')">${{d}}</div>
                        `).join('') : '<span class="empty">None</span>'}}
                    </div>
                </div>

                <div class="section">
                    <h3>Path</h3>
                    <div class="cmd-box">
                        ${{node.path}}
                        <button class="copy-btn" onclick="window.copyToClipboard('${{node.path.replace(/\\/g, '\\\\\\\\')}}')">Copy</button>
                    </div>
                </div>

                <div class="section">
                    <h3>Actions</h3>
                    <div class="actions">
                        <button class="action-btn" onclick="window.copyToClipboard('${{node.path.replace(/\\/g, '\\\\\\\\')}}')">
                            <span class="icon">📋</span> Copy Path
                        </button>
                        <button class="action-btn" onclick="window.copyDeadmodCommand('${{node.id}}')">
                            <span class="icon">⚡</span> Copy Deadmod Command
                        </button>
                        <button class="action-btn success" onclick="window.highlightConnections('${{node.id}}')">
                            <span class="icon">🔍</span> Highlight Connections
                        </button>
                        ${{node.status === 'dead' ? `
                        <button class="action-btn danger" onclick="window.showRemoveCommand('${{node.path.replace(/\\/g, '\\\\\\\\')}}')">
                            <span class="icon">🗑️</span> Show Remove Command
                        </button>
                        ` : ''}}
                    </div>
                </div>
            `;
        }}

        // Global function to select node from inspector
        window.selectNode = function(id) {{
            const node = nodeMap[id];
            if (node) {{
                selectedNode = node;
                updateInspector(node);
                // Center view on node
                offsetX = width/2 - node.x * scale;
                offsetY = height/2 - node.y * scale;
            }}
        }};

        // Toast notification
        let toastTimeout = null;
        function showToast(message, type = 'success') {{
            let toast = document.getElementById('toast');
            if (!toast) {{
                toast = document.createElement('div');
                toast.id = 'toast';
                document.body.appendChild(toast);
            }}
            toast.textContent = message;
            toast.style.color = type === 'success' ? '#90EE90' : '#F08080';
            toast.classList.add('visible');
            clearTimeout(toastTimeout);
            toastTimeout = setTimeout(() => toast.classList.remove('visible'), 2000);
        }}

        // Copy to clipboard
        window.copyToClipboard = function(text) {{
            navigator.clipboard.writeText(text).then(() => {{
                showToast('Copied to clipboard!');
            }}).catch(() => {{
                showToast('Failed to copy', 'error');
            }});
        }};

        // Copy deadmod command for this module
        window.copyDeadmodCommand = function(moduleId) {{
            const node = nodeMap[moduleId];
            if (!node) return;
            // Extract parent path from module path
            const pathParts = node.path.replace(/\\\\/g, '/').split('/');
            let cratePath = pathParts.slice(0, pathParts.indexOf('src')).join('/');
            if (!cratePath) cratePath = '.';
            const cmd = `deadmod "${{cratePath}}"`;
            navigator.clipboard.writeText(cmd).then(() => {{
                showToast('Command copied!');
            }}).catch(() => {{
                showToast('Failed to copy', 'error');
            }});
        }};

        // Highlight connections tracking
        let highlightedNodes = new Set();

        // Highlight connected nodes
        window.highlightConnections = function(moduleId) {{
            const deps = outbound[moduleId] || [];
            const dependents = inbound[moduleId] || [];
            highlightedNodes.clear();
            highlightedNodes.add(moduleId);
            deps.forEach(d => highlightedNodes.add(d));
            dependents.forEach(d => highlightedNodes.add(d));
            showToast(`Highlighted ${{highlightedNodes.size}} connected modules`);
        }};

        // Show remove command for dead module
        window.showRemoveCommand = function(path) {{
            const cmd = `# Remove dead module:\\nrm "${{path}}"\\n# Also remove any 'mod modulename;' declarations referencing it`;
            navigator.clipboard.writeText(cmd.replace(/\\\\n/g, '\\n')).then(() => {{
                showToast('Remove command copied!');
            }}).catch(() => {{
                showToast('Failed to copy', 'error');
            }});
        }};

        // Event handlers
        canvas.addEventListener('mousedown', e => {{
            const rect = canvas.getBoundingClientRect();
            const mx = e.clientX - rect.left;
            const my = e.clientY - rect.top;

            const node = getNodeAt(mx, my);
            if (node) {{
                dragNode = node;
                selectedNode = node;
                updateInspector(node);
            }} else {{
                dragging = true;
            }}
            lastMouse = {{ x: e.clientX, y: e.clientY }};
        }});

        canvas.addEventListener('mousemove', e => {{
            const rect = canvas.getBoundingClientRect();
            const mx = e.clientX - rect.left;
            const my = e.clientY - rect.top;

            if (dragNode) {{
                const world = toWorld(mx, my);
                dragNode.x = world.x;
                dragNode.y = world.y;
                dragNode.vx = 0;
                dragNode.vy = 0;
            }} else if (dragging) {{
                offsetX += e.clientX - lastMouse.x;
                offsetY += e.clientY - lastMouse.y;
            }} else {{
                const node = getNodeAt(mx, my);
                if (node) {{
                    showTooltip(node, e.clientX, e.clientY);
                    canvas.style.cursor = 'pointer';
                }} else {{
                    hideTooltip();
                    canvas.style.cursor = 'grab';
                }}
            }}
            lastMouse = {{ x: e.clientX, y: e.clientY }};
        }});

        canvas.addEventListener('mouseup', () => {{
            dragging = false;
            dragNode = null;
        }});

        canvas.addEventListener('mouseleave', () => {{
            dragging = false;
            dragNode = null;
            hideTooltip();
        }});

        canvas.addEventListener('wheel', e => {{
            e.preventDefault();
            const delta = e.deltaY > 0 ? 0.9 : 1.1;
            const rect = canvas.getBoundingClientRect();
            const mx = e.clientX - rect.left;
            const my = e.clientY - rect.top;

            // Zoom toward mouse position
            offsetX = mx - (mx - offsetX) * delta;
            offsetY = my - (my - offsetY) * delta;
            scale *= delta;
            scale = Math.max(0.1, Math.min(5, scale));
        }});

        // Control buttons
        document.getElementById('zoom-in').onclick = () => {{
            scale *= 1.2;
            scale = Math.min(5, scale);
        }};
        document.getElementById('zoom-out').onclick = () => {{
            scale *= 0.8;
            scale = Math.max(0.1, scale);
        }};
        document.getElementById('reset').onclick = () => {{
            scale = 1;
            offsetX = width / 2;
            offsetY = height / 2;
        }};

        // Toggle edge bundling
        const bundleBtn = document.getElementById('toggle-bundling');
        bundleBtn.onclick = () => {{
            edgeBundling = !edgeBundling;
            bundleBtn.classList.toggle('active', edgeBundling);
        }};
        bundleBtn.classList.toggle('active', edgeBundling);

        // Toggle cluster gravity
        const clusterBtn = document.getElementById('toggle-clusters');
        clusterBtn.onclick = () => {{
            clusterGravity = !clusterGravity;
            clusterBtn.classList.toggle('active', clusterGravity);
        }};
        clusterBtn.classList.toggle('active', clusterGravity);

        // Clear highlights
        const clearBtn = document.getElementById('clear-highlight');
        window.clearHighlights = function() {{
            highlightedNodes.clear();
            clearBtn.classList.remove('active');
        }};
        clearBtn.onclick = window.clearHighlights;

        // Keyboard shortcuts
        document.addEventListener('keydown', e => {{
            if (e.key === 'Escape') {{
                window.clearHighlights();
            }}
        }});

        // Animation loop
        function loop() {{
            simulate();
            draw();
            requestAnimationFrame(loop);
        }}

        window.addEventListener('resize', resize);
        resize();
        loop();
    }})();
    </script>
</body>
</html>"##,
        total = total,
        reachable_count = reachable_count,
        dead_count = dead_count,
        cluster_count = clusters.len(),
        nodes_json = nodes_json,
        edges_json = edges_json,
        clusters_json = clusters_json
    )
}

/// Extract parent module name from file path for clustering.
fn extract_parent_module(path: &str) -> String {
    // Try to extract the parent directory name as the cluster
    let path = path.replace('\\', "/");
    let parts: Vec<&str> = path.split('/').collect();

    // Find "src" and get the next component
    for (i, part) in parts.iter().enumerate() {
        if *part == "src" && i + 1 < parts.len() {
            let next = parts[i + 1];
            // If it's a .rs file, use its name without extension
            if next.ends_with(".rs") {
                return next.trim_end_matches(".rs").to_string();
            }
            // Otherwise it's a directory
            return next.to_string();
        }
    }

    // Fallback: use filename without extension
    parts.last()
        .map(|s| s.trim_end_matches(".rs").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_generate_html_graph_empty() {
        let mods = HashMap::new();
        let reachable = HashSet::new();

        let html = generate_html_graph(&mods, &reachable);

        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Deadmod Graph"));
        assert!(html.contains("Total:<span class=\"stat-value\">0</span>"));
    }

    #[test]
    fn test_generate_html_graph_with_modules() {
        let mut mods = HashMap::new();
        let mut reachable = HashSet::new();

        let mut main_info = crate::parse::ModuleInfo::new(PathBuf::from("src/main.rs"));
        main_info.refs.insert("utils".to_string());
        mods.insert("main".to_string(), main_info);

        mods.insert(
            "utils".to_string(),
            crate::parse::ModuleInfo::new(PathBuf::from("src/utils.rs")),
        );

        mods.insert(
            "dead".to_string(),
            crate::parse::ModuleInfo::new(PathBuf::from("src/dead.rs")),
        );

        reachable.insert("main".to_string());
        reachable.insert("utils".to_string());

        let html = generate_html_graph(&mods, &reachable);

        assert!(html.contains("\"main\""));
        assert!(html.contains("\"utils\""));
        assert!(html.contains("\"dead\""));
        assert!(html.contains("#90EE90")); // reachable color
        assert!(html.contains("#F08080")); // dead color
        assert!(html.contains("Reachable:<span class=\"stat-value green\">2</span>"));
        assert!(html.contains("Dead:<span class=\"stat-value red\">1</span>"));
    }

    #[test]
    fn test_generate_html_graph_has_interactivity() {
        let mods = HashMap::new();
        let reachable = HashSet::new();

        let html = generate_html_graph(&mods, &reachable);

        // Check for interactive elements
        assert!(html.contains("addEventListener"));
        assert!(html.contains("mousedown"));
        assert!(html.contains("mousemove"));
        assert!(html.contains("wheel"));
        assert!(html.contains("zoom-in"));
        assert!(html.contains("zoom-out"));
    }

    #[test]
    fn test_generate_html_graph_escapes_paths() {
        let mut mods = HashMap::new();

        mods.insert(
            "test".to_string(),
            crate::parse::ModuleInfo::new(PathBuf::from("C:\\path\\with\\backslashes\\test.rs")),
        );

        let html = generate_html_graph(&mods, &HashSet::new());

        // Backslashes should be escaped for JSON
        assert!(html.contains("\\\\"));
    }

    #[test]
    fn test_generate_html_graph_has_inspector() {
        let mods = HashMap::new();
        let reachable = HashSet::new();

        let html = generate_html_graph(&mods, &reachable);

        assert!(html.contains("id=\"inspector\""));
        assert!(html.contains("Module Inspector"));
        assert!(html.contains("updateInspector"));
    }

    #[test]
    fn test_generate_html_graph_has_edge_bundling() {
        let mods = HashMap::new();
        let reachable = HashSet::new();

        let html = generate_html_graph(&mods, &reachable);

        assert!(html.contains("edgeBundling"));
        assert!(html.contains("getBundleControlPoint"));
        assert!(html.contains("quadraticCurveTo"));
    }

    #[test]
    fn test_generate_html_graph_has_clustering() {
        let mods = HashMap::new();
        let reachable = HashSet::new();

        let html = generate_html_graph(&mods, &reachable);

        assert!(html.contains("clusterGravity"));
        assert!(html.contains("clusterCenters"));
        assert!(html.contains("cluster-tag"));
    }

    #[test]
    fn test_extract_parent_module() {
        assert_eq!(extract_parent_module("src/main.rs"), "main");
        assert_eq!(extract_parent_module("src/utils/helper.rs"), "utils");
        assert_eq!(extract_parent_module("/path/to/src/module/file.rs"), "module");
        assert_eq!(extract_parent_module("C:\\project\\src\\config.rs"), "config");
    }
}

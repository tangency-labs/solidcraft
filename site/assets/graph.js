// solidcore dependency graph (graph.html): lays out assets/graph.json with d3-force
// (vendor/d3-force.bundle.min.js) into an SVG with pan/zoom, hover, search, module filter
// and click-to-highlight. Without JS the page keeps its server-rendered lists.
(function () {
  "use strict";
  var ui = document.querySelector(".graphui");
  var svg = document.getElementById("graph");
  if (!ui || !svg || typeof d3 === "undefined" || !window.fetch) return;
  var NS = "http://www.w3.org/2000/svg";
  var COL = 36, ROW = 20, PAD = 24, LABEL = 150;

  function el(name, attrs, parent) {
    var e = document.createElementNS(NS, name);
    for (var k in attrs) e.setAttribute(k, attrs[k]);
    if (parent) parent.appendChild(e);
    return e;
  }

  fetch("assets/graph.json").then(function (r) { return r.json(); }).then(draw).catch(function () {});

  function draw(data) {
    ui.hidden = false;
    var nodes = data.nodes, byId = {};
    nodes.forEach(function (n, i) { n.i = i; n.up = []; n.down = []; byId[n.id] = n; });
    var edges = [];
    nodes.forEach(function (n) {
      n.deps.forEach(function (d) {
        var s = byId[d];
        if (!s) return;
        edges.push({ source: s, target: n });
        n.up.push(s); s.down.push(n);
      });
    });

    // Rows: one band per module, tall enough for its busiest layer.
    var bands = [], y = PAD;
    data.areas.forEach(function (a) {
      var per = {};
      nodes.forEach(function (n) { if (n.area === a) per[n.layer] = (per[n.layer] || 0) + 1; });
      var most = Math.max.apply(null, Object.keys(per).map(function (k) { return per[k]; }).concat([1]));
      var h = Math.max(2, most) * ROW + ROW;
      bands.push({ area: a, y0: y, y1: y + h, c: y + h / 2 });
      y += h;
    });
    var bandOf = {};
    bands.forEach(function (b) { bandOf[b.area] = b; });
    var maxLayer = Math.max.apply(null, nodes.map(function (n) { return n.layer; }));
    var W = LABEL + PAD * 2 + COL * maxLayer, H = y + PAD;
    function lx(layer) { return LABEL + PAD + COL * layer; }
    function rad(n) { return 3.5 + Math.sqrt(n.lines) / 9; }

    nodes.forEach(function (n, k) {
      var b = bandOf[n.area];
      n.x = lx(n.layer);
      n.y = b ? b.c + ((k % 5) - 2) * 4 : H / 2;
    });
    var sim = d3.forceSimulation(nodes)
      .force("x", d3.forceX(function (n) { return lx(n.layer); }).strength(1))
      .force("y", d3.forceY(function (n) { return bandOf[n.area] ? bandOf[n.area].c : H / 2; }).strength(0.35))
      .force("link", d3.forceLink(edges).strength(0.0005))
      .force("collide", d3.forceCollide(function (n) { return rad(n) + 2; }).iterations(2))
      .stop();
    for (var t = 0; t < 300; t++) sim.tick();
    nodes.forEach(function (n) {
      var b = bandOf[n.area];
      if (b) n.y = Math.min(b.y1 - rad(n), Math.max(b.y0 + rad(n), n.y));
    });

    // ---- drawing
    var vp = el("g", { "class": "vp" }, svg);
    var gb = el("g", { "class": "bands" }, vp);
    bands.forEach(function (b, k) {
      el("rect", { x: 0, y: b.y0, width: W, height: b.y1 - b.y0, "class": k % 2 ? "band odd" : "band" }, gb);
      el("text", { x: 8, y: b.c + 4, "class": "blabel" }, gb).textContent = b.area;
    });
    var ge = el("g", { "class": "edges" }, vp);
    edges.forEach(function (e) {
      e.el = el("line", { x1: e.source.x.toFixed(1), y1: e.source.y.toFixed(1),
        x2: e.target.x.toFixed(1), y2: e.target.y.toFixed(1) }, ge);
    });
    var gn = el("g", { "class": "nodes" }, vp);
    nodes.forEach(function (n) {
      var g = el("g", { "class": "n k-" + n.kind + " s-" + n.status, "data-i": n.i,
        transform: "translate(" + n.x.toFixed(1) + "," + n.y.toFixed(1) + ")" }, gn);
      if (n.ready) el("circle", { r: rad(n) + 3.5, "class": "ring" }, g);
      el("circle", { r: rad(n), "class": "dot" }, g);
      n.el = g;
    });

    // ---- view box: pan and zoom
    var view = { x: 0, y: 0, w: W, h: H };
    function apply() { svg.setAttribute("viewBox", view.x + " " + view.y + " " + view.w + " " + view.h); }
    function fit() {
      var box = svg.getBoundingClientRect();
      var aspect = box.width / Math.max(1, box.height);
      view.w = Math.max(W, H * aspect); view.h = view.w / aspect;
      view.x = (W - view.w) / 2; view.y = (H - view.h) / 2;
      apply();
    }
    function toView(cx, cy) {
      var box = svg.getBoundingClientRect();
      return { x: view.x + (cx - box.left) * view.w / box.width, y: view.y + (cy - box.top) * view.h / box.height };
    }
    function zoom(f, cx, cy) {
      var p = toView(cx, cy);
      var nw = Math.min(W * 4, Math.max(120, view.w * f));
      f = nw / view.w;
      view.x = p.x - (p.x - view.x) * f; view.y = p.y - (p.y - view.y) * f;
      view.w *= f; view.h *= f;
      apply();
    }
    fit();
    window.addEventListener("resize", function () {
      var box = svg.getBoundingClientRect();
      view.h = view.w * box.height / Math.max(1, box.width);
      apply();
    });
    svg.addEventListener("wheel", function (ev) {
      ev.preventDefault();
      zoom(Math.exp(ev.deltaY * 0.0015), ev.clientX, ev.clientY);
    }, { passive: false });
    var pts = {}, drag = null, moved = 0;
    svg.addEventListener("pointerdown", function (ev) {
      pts[ev.pointerId] = { x: ev.clientX, y: ev.clientY };
      moved = 0;
      drag = { x: ev.clientX, y: ev.clientY };
      svg.setPointerCapture(ev.pointerId);
    });
    svg.addEventListener("pointermove", function (ev) {
      var prev = pts[ev.pointerId];
      if (!prev) { hover(ev); return; }
      var ids = Object.keys(pts);
      if (ids.length === 2) {
        var o = pts[ids[0] == ev.pointerId ? ids[1] : ids[0]];
        var d0 = Math.hypot(prev.x - o.x, prev.y - o.y), d1 = Math.hypot(ev.clientX - o.x, ev.clientY - o.y);
        if (d0 > 0 && d1 > 0) zoom(d0 / d1, (ev.clientX + o.x) / 2, (ev.clientY + o.y) / 2);
        moved += 10;
      } else {
        var box = svg.getBoundingClientRect();
        view.x -= (ev.clientX - prev.x) * view.w / box.width;
        view.y -= (ev.clientY - prev.y) * view.h / box.height;
        moved += Math.abs(ev.clientX - prev.x) + Math.abs(ev.clientY - prev.y);
        apply();
      }
      pts[ev.pointerId] = { x: ev.clientX, y: ev.clientY };
    });
    function up(ev) {
      var wasTap = pts[ev.pointerId] && Object.keys(pts).length === 1 && moved < 6;
      delete pts[ev.pointerId];
      if (ev.type === "pointerup" && wasTap) {
        var hit = document.elementFromPoint(ev.clientX, ev.clientY);
        var g = hit && hit.closest ? hit.closest("g.n") : null;
        select(g ? nodes[+g.getAttribute("data-i")] : null);
      }
    }
    svg.addEventListener("pointerup", up);
    svg.addEventListener("pointercancel", up);

    // ---- tooltip
    var tip = svg.parentNode.querySelector(".tip");
    function hover(ev) {
      var g = ev.target.closest ? ev.target.closest("g.n") : null;
      if (!g) { tip.hidden = true; return; }
      var n = nodes[+g.getAttribute("data-i")];
      tip.textContent = "";
      [n.id, n.status + (n.kind === "ready" ? " (ready to claim)" : n.ready && n.kind !== "done" ? " (dependencies done)" : "") + " · " + n.area + " · layer " + n.layer,
       n.claim ? "claimed: " + n.claim : "",
       "blocked by " + n.blocked_by + " unfinished " + (n.blocked_by === 1 ? "dependency" : "dependencies"),
       n.blocking ? "blocking " + n.blocking + " unfinished units" : ""].forEach(function (s, i) {
        if (!s) return;
        var d = document.createElement("div"); d.textContent = s;
        if (i === 0) d.style.fontWeight = "600";
        tip.appendChild(d);
      });
      tip.hidden = false;
      var host = svg.parentNode.getBoundingClientRect();
      var left = ev.clientX - host.left + 12, top = ev.clientY - host.top + 12;
      if (left + tip.offsetWidth > host.width - 4) left = ev.clientX - host.left - tip.offsetWidth - 12;
      if (top + tip.offsetHeight > host.height - 4) top = ev.clientY - host.top - tip.offsetHeight - 12;
      tip.style.left = Math.max(4, left) + "px"; tip.style.top = Math.max(4, top) + "px";
    }
    svg.addEventListener("pointerleave", function () { tip.hidden = true; });

    // ---- selection: upstream dependencies and downstream dependents
    var info = document.getElementById("g-info");
    var selected = null;
    function closure(n, key) {
      var seen = {}, stack = [n];
      while (stack.length) {
        stack.pop()[key].forEach(function (m) { if (!seen[m.id]) { seen[m.id] = true; stack.push(m); } });
      }
      return seen;
    }
    function link(id) {
      var a = document.createElement("a");
      a.href = "units/" + encodeURIComponent(id) + ".html";
      var c = document.createElement("code"); c.textContent = id; a.appendChild(c);
      return a;
    }
    function select(n) {
      if (n && selected === n) n = null;
      selected = n;
      nodes.forEach(function (m) { m.el.classList.remove("sel", "up", "down"); });
      edges.forEach(function (e) { e.el.classList.remove("up", "down"); });
      svg.classList.toggle("focus", !!n);
      info.textContent = "";
      if (!n) {
        var p = document.createElement("p"); p.className = "muted"; p.textContent = "No unit selected."; info.appendChild(p);
        return;
      }
      var ups = closure(n, "up"), downs = closure(n, "down");
      n.el.classList.add("sel");
      nodes.forEach(function (m) {
        if (ups[m.id]) m.el.classList.add("up");
        if (downs[m.id]) m.el.classList.add("down");
      });
      edges.forEach(function (e) {
        if ((ups[e.target.id] || e.target === n) && ups[e.source.id]) e.el.classList.add("up");
        if ((downs[e.source.id] || e.source === n) && downs[e.target.id]) e.el.classList.add("down");
      });
      var h = document.createElement("p");
      h.appendChild(link(n.id));
      h.appendChild(document.createTextNode(" " + n.status + (n.kind === "ready" ? ", ready to claim" : n.ready && n.kind !== "done" ? ", dependencies done" : "") +
        " · " + n.area + " · layer " + n.layer + " · " + n.lines.toLocaleString("en-US") + " lines"));
      info.appendChild(h);
      if (n.claim) { var c = document.createElement("p"); c.textContent = "Claimed: " + n.claim; info.appendChild(c); }
      var s = document.createElement("p");
      s.textContent = Object.keys(ups).length + " upstream dependencies (transitive), " +
        Object.keys(downs).length + " downstream dependents; blocking " + n.blocking + " unfinished units.";
      info.appendChild(s);
      var open = n.up.filter(function (m) { return m.kind !== "done"; });
      if (open.length) {
        var b = document.createElement("p");
        b.appendChild(document.createTextNode("Waiting on: "));
        open.forEach(function (m, i) { if (i) b.appendChild(document.createTextNode(", ")); b.appendChild(link(m.id)); });
        info.appendChild(b);
      }
    }

    // ---- search and module filter
    var q = document.getElementById("g-search"), area = document.getElementById("g-area");
    function filter() {
      var s = q.value.trim().toLowerCase(), a = area.value;
      svg.classList.toggle("searching", !!s);
      nodes.forEach(function (n) {
        n.el.classList.toggle("hit", !!s && n.id.toLowerCase().indexOf(s) >= 0);
        n.el.classList.toggle("out", !!a && n.area !== a);
      });
      edges.forEach(function (e) {
        e.el.classList.toggle("out", !!a && (e.source.area !== a || e.target.area !== a));
      });
    }
    q.addEventListener("input", filter);
    area.addEventListener("change", function () {
      filter();
      var b = bandOf[area.value];
      if (b) {
        // fit the module's band, from its first to its last unit's column
        var xs = nodes.filter(function (n) { return n.area === area.value; }).map(function (n) { return n.x; });
        var x0 = Math.min.apply(null, xs) - COL, x1 = Math.max.apply(null, xs) + COL;
        var y0 = b.y0 - ROW, y1 = b.y1 + ROW;
        var box = svg.getBoundingClientRect(), aspect = box.width / Math.max(1, box.height);
        view.w = Math.max(x1 - x0, (y1 - y0) * aspect); view.h = view.w / aspect;
        view.x = (x0 + x1 - view.w) / 2; view.y = (y0 + y1 - view.h) / 2;
        apply();
      } else fit();
    });
    q.addEventListener("keydown", function (ev) {
      if (ev.key !== "Enter") return;
      var s = q.value.trim().toLowerCase();
      var hit = nodes.filter(function (n) { return n.id.toLowerCase().indexOf(s) >= 0; })[0];
      if (!s || !hit) return;
      ev.preventDefault();
      if (selected !== hit) select(hit);
      view.x = hit.x - view.w / 2; view.y = hit.y - view.h / 2;
      apply();
    });
    filter();
  }
})();

// solidcore progress site: table sorting, filtering, chart picker and chart tooltips. No dependencies.
(function () {
  "use strict";

  // ---- sortable tables: <th data-sort="num|text">; a cell's data-v overrides its text.
  function cellValue(td, kind) {
    if (!td) return kind === "num" ? -Infinity : "";
    var v = td.hasAttribute("data-v") ? td.getAttribute("data-v") : td.textContent.trim();
    if (kind === "num") {
      var n = parseFloat(String(v).replace(/,/g, ""));
      return isNaN(n) ? -Infinity : n;
    }
    return String(v).toLowerCase();
  }
  document.querySelectorAll("table.sortable").forEach(function (table) {
    var ths = table.tHead ? table.tHead.rows[0].cells : [];
    Array.prototype.forEach.call(ths, function (th, col) {
      var kind = th.getAttribute("data-sort");
      if (!kind) return;
      th.tabIndex = 0;
      function sort() {
        var asc = th.getAttribute("aria-sort") !== "ascending";
        Array.prototype.forEach.call(ths, function (o) { o.removeAttribute("aria-sort"); });
        th.setAttribute("aria-sort", asc ? "ascending" : "descending");
        var body = table.tBodies[0];
        var rows = Array.prototype.slice.call(body.rows);
        rows.forEach(function (r, i) { r._i = i; });
        rows.sort(function (a, b) {
          var x = cellValue(a.cells[col], kind), y = cellValue(b.cells[col], kind);
          var c = x < y ? -1 : x > y ? 1 : 0;
          return (asc ? c : -c) || a._i - b._i;
        });
        rows.forEach(function (r) { body.appendChild(r); });
      }
      th.addEventListener("click", sort);
      th.addEventListener("keydown", function (e) { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); sort(); } });
    });
  });

  // ---- filters: <div class="filters" data-table=ID | data-list=ID>; selects match data-<name>.
  document.querySelectorAll(".filters").forEach(function (box) {
    var id = box.getAttribute("data-table") || box.getAttribute("data-list");
    var root = document.getElementById(id);
    if (!root) return;
    var items = box.hasAttribute("data-table") ? root.tBodies[0].rows : root.children;
    var controls = box.querySelectorAll("input, select");
    var count = box.querySelector(".count");
    var params = new URLSearchParams(location.search);
    controls.forEach(function (c) {
      var v = params.get(c.name);
      if (v !== null) c.value = v;
    });
    function apply() {
      var shown = 0;
      Array.prototype.forEach.call(items, function (el) {
        var ok = true;
        controls.forEach(function (c) {
          var v = c.value.trim();
          if (!v) return;
          if (c.name === "q") {
            if (el.textContent.toLowerCase().indexOf(v.toLowerCase()) < 0) ok = false;
          } else if (el.getAttribute("data-" + c.name) !== v) ok = false;
        });
        el.hidden = !ok;
        if (ok) shown++;
      });
      if (count) count.textContent = shown + " of " + items.length + " shown";
      var p = new URLSearchParams();
      controls.forEach(function (c) { if (c.value.trim()) p.set(c.name, c.value.trim()); });
      var qs = p.toString();
      history.replaceState(null, "", location.pathname + (qs ? "?" + qs : "") + location.hash);
    }
    controls.forEach(function (c) { c.addEventListener("input", apply); c.addEventListener("change", apply); });
    apply();
  });

  // ---- chart picker: <select data-charts=ID> shows the .chartset whose data-area matches.
  document.querySelectorAll("select[data-charts]").forEach(function (sel) {
    var root = document.getElementById(sel.getAttribute("data-charts"));
    if (!root) return;
    var pick = sel.closest(".chartpick");
    if (pick) pick.hidden = false;
    function show() {
      root.querySelectorAll(".chartset").forEach(function (c) {
        c.hidden = c.getAttribute("data-area") !== sel.value;
      });
    }
    sel.addEventListener("change", show);
    show();
  });

  // ---- chart crosshair + tooltip (one per svg.chart; several per page are fine)
  document.querySelectorAll("svg.chart[data-points]").forEach(function (svg) {
    var pts = JSON.parse(svg.getAttribute("data-points"));
    var x0 = +svg.getAttribute("data-x0"), x1 = +svg.getAttribute("data-x1");
    var L = +svg.getAttribute("data-l"), R = +svg.getAttribute("data-r"), W = +svg.getAttribute("data-w");
    var cross = svg.querySelector(".cross"), hit = svg.querySelector(".hit");
    var tip = svg.parentNode.querySelector(".tip");
    function sx(t) { return L + (W - L - R) * (t - x0) / Math.max(1, x1 - x0); }
    function fmt(t) {
      return new Date(t * 1000).toISOString().slice(0, 16).replace("T", " ") + " UTC";
    }
    function move(ev) {
      var box = svg.getBoundingClientRect();
      var vx = (ev.clientX - box.left) * W / box.width;
      var best = pts[0], bd = Infinity;
      pts.forEach(function (p) { var d = Math.abs(sx(p[0]) - vx); if (d < bd) { bd = d; best = p; } });
      var px = sx(best[0]);
      cross.setAttribute("x1", px); cross.setAttribute("x2", px); cross.setAttribute("visibility", "visible");
      tip.hidden = false;
      tip.textContent = "";
      [fmt(best[0]), "green: " + best[1].toFixed(1) + "%", "green + partial: " + best[2].toFixed(1) + "%",
       best[3] + " / " + best[4] + " units"].forEach(function (s, i) {
        var d = document.createElement("div"); d.textContent = s; if (i === 0) d.style.fontWeight = "600"; tip.appendChild(d);
      });
      var left = px * box.width / W + 8;
      var host = svg.parentNode.getBoundingClientRect().width;
      if (left + tip.offsetWidth > host - 4) left = px * box.width / W - tip.offsetWidth - 8;
      tip.style.left = Math.max(4, left) + "px";
      tip.style.top = "12px";
    }
    function leave() { cross.setAttribute("visibility", "hidden"); tip.hidden = true; }
    hit.addEventListener("pointermove", move);
    hit.addEventListener("pointerdown", move);
    hit.addEventListener("pointerleave", leave);
  });
})();

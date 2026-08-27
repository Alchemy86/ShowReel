(() => {
  "use strict";

  // ---- tiny DOM helpers ---------------------------------------------------
  const $ = (id) => document.getElementById(id);
  const el = (tag, cls, text) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined) e.textContent = text;
    return e;
  };
  const fmtTime = (s) => {
    s = Math.max(0, s || 0);
    const m = Math.floor(s / 60);
    const rem = s - m * 60;
    return `${m}:${rem.toFixed(2).padStart(5, "0")}`;
  };
  const truncate = (s, n) => (s && s.length > n ? s.slice(0, n - 1) + "…" : s || "");

  // A handful of muted scene colours, cycling — distinct enough to tell scenes
  // apart on the scrubber without competing with the accent, which is reserved
  // for the playhead, the loop band and transition marks.
  const SCENE_COLOURS = ["#2b3444", "#233a3c", "#3a2f3d", "#2f3a2b", "#3d3226", "#26333d"];

  // ---- state ----------------------------------------------------------------
  let state = null; // last /api/state payload
  let currentTime = 0;
  let duration = 0;
  let fps = 30;
  let playing = false;
  let playAnchorWall = 0;
  let playAnchorTime = 0;
  let pendingFrame = false;
  let frameTimes = [];
  let loopIn = null;
  let loopOut = null;
  let rafId = null;

  const seek = $("seek");
  const frameImg = $("frame");
  const stage = $("stage");
  const playhead = $("playhead");
  const loopBand = $("loop-band");

  // ---- fetching state ---------------------------------------------------------

  async function pollState() {
    try {
      const res = await fetch("/api/state", { cache: "no-store" });
      const data = await res.json();
      setConn("ok");
      if (!state || data.version !== state.version) {
        applyState(data);
      } else {
        state = data;
      }
    } catch (e) {
      setConn("down");
    }
  }

  function setConn(mode) {
    const node = $("conn-status");
    node.className = mode;
    node.textContent = mode === "ok" ? "● live" : mode === "down" ? "● disconnected" : "● stale";
  }

  function applyState(data) {
    const firstLoad = state === null;
    state = data;
    duration = data.duration || 0;
    fps = (data.film && data.film.fps) || 30;
    seek.max = String(Math.max(duration, 0.001));

    $("film-title").textContent = data.film ? (data.film.title || data.path) : data.path;
    document.title = (data.film && data.film.title ? data.film.title : data.path) + " — ShowReel Studio";
    $("film-stats").textContent = data.film
      ? `${data.film.width}×${data.film.height} · ${data.film.fps}fps · ${fmtTime(duration)}`
      : "";

    renderErrors(data);
    renderScrubber(data);
    renderStructure(data);

    if (firstLoad) {
      currentTime = 0;
    } else {
      currentTime = Math.min(currentTime, duration);
    }
    seek.value = String(currentTime);
    updatePlayhead();
    updateTimecode();
    if (data.film) {
      renderFrame(currentTime, true);
    }
  }

  function renderErrors(data) {
    const banner = $("error-banner");
    const title = $("error-banner-title");
    const list = $("error-list");
    list.innerHTML = "";
    const errs = [];
    if (data.parseError) errs.push(data.parseError);
    for (const e of data.validationErrors || []) errs.push(e);
    if (errs.length === 0) {
      banner.hidden = true;
      return;
    }
    title.textContent = data.parseError
      ? "The film file does not parse:"
      : `${errs.length} problem${errs.length === 1 ? "" : "s"} — the last frame that did parse is still shown below:`;
    for (const e of errs) list.appendChild(el("li", null, e));
    banner.hidden = false;
  }

  // ---- scrubber ------------------------------------------------------------------

  function renderScrubber(data) {
    const scenes = $("scenes");
    const labels = $("scene-labels");
    scenes.innerHTML = "";
    labels.innerHTML = "";
    if (!data.film || duration <= 0) return;

    const placements = data.placements || [];
    const scenesArr = allScenes(data.film);
    placements.forEach((p, i) => {
      const scene = scenesArr[p.index];
      const block = el("div", "scene-block");
      const widthPct = ((p.duration / duration) * 100).toFixed(3);
      block.style.width = widthPct + "%";
      block.style.background = SCENE_COLOURS[i % SCENE_COLOURS.length];
      block.title = `${scene.name || "scene " + (p.index + 1)} — ${fmtTime(p.start)}–${fmtTime(p.end)}`;
      block.textContent = scene.name || `scene ${p.index + 1}`;
      scenes.appendChild(block);

      if (i > 0) {
        const link = data.film.then[i - 1];
        const mark = el("div", "transition-mark");
        mark.style.left = ((p.start / duration) * 100).toFixed(3) + "%";
        mark.title = `${link.transition.presentation.kind} · ${link.transition.duration}s`;
        scenes.appendChild(mark);
      }

      const label = el("span", null, fmtTime(p.start));
      labels.appendChild(label);
    });
    labels.appendChild(el("span", null, fmtTime(duration)));
  }

  function allScenes(film) {
    return [film.opening, ...(film.then || []).map((l) => l.scene)];
  }

  function updatePlayhead() {
    const pct = duration > 0 ? (currentTime / duration) * 100 : 0;
    playhead.style.left = pct + "%";
    seek.value = String(currentTime);
  }

  function updateTimecode() {
    $("timecode").innerHTML = `${fmtTime(currentTime)} <span id="timecode-sep">/</span> ${fmtTime(duration)}`;
  }

  function updateLoopBand() {
    if (loopIn == null || loopOut == null || duration <= 0) {
      loopBand.hidden = true;
      return;
    }
    const a = Math.min(loopIn, loopOut);
    const b = Math.max(loopIn, loopOut);
    loopBand.style.left = ((a / duration) * 100).toFixed(3) + "%";
    loopBand.style.width = (((b - a) / duration) * 100).toFixed(3) + "%";
    loopBand.hidden = false;
  }

  // ---- structure panel --------------------------------------------------------------

  function renderStructure(data) {
    const list = $("scene-list");
    const openState = new Set(
      [...list.querySelectorAll(".scene-card.open")].map((c) => c.dataset.index)
    );
    list.innerHTML = "";
    if (!data.film) return;
    const placements = data.placements || [];
    const scenesArr = allScenes(data.film);

    placements.forEach((p, i) => {
      const scene = scenesArr[p.index];
      const card = el("div", "scene-card");
      card.dataset.index = String(i);
      card.style.borderLeftColor = SCENE_COLOURS[i % SCENE_COLOURS.length];
      if (openState.has(String(i)) || openState.size === 0) card.classList.add("open");

      const head = el("div", "scene-card-head");
      const name = el("div", "scene-card-name");
      name.appendChild(el("span", "scene-card-toggle", "▸"));
      name.appendChild(document.createTextNode(" " + (scene.name || `scene ${p.index + 1}`)));
      head.appendChild(name);
      head.appendChild(el("div", "scene-card-time", `${fmtTime(p.start)}–${fmtTime(p.end)}`));
      head.onclick = () => card.classList.toggle("open");
      card.appendChild(head);

      if (i > 0) {
        const link = data.film.then[i - 1];
        card.appendChild(
          el(
            "div",
            "scene-card-transition",
            `via ${link.transition.presentation.kind} · ${link.transition.duration}s`
          )
        );
      }

      const layers = el("div", "scene-card-layers");
      for (const layer of scene.layers || []) {
        const row = el("div", "layer-row");
        row.appendChild(el("span", "layer-kind", layer.type));
        row.appendChild(el("span", "layer-detail", layerDetail(layer)));
        const dur = layer.duration != null ? `${layer.duration}s` : "→ end";
        row.appendChild(el("span", "layer-timing", `${layer.from || 0}s ${dur}`));
        layers.appendChild(row);
      }
      if ((scene.layers || []).length === 0) {
        layers.appendChild(el("div", "layer-row", "(no layers)"));
      }
      card.appendChild(layers);
      list.appendChild(card);
    });

    renderAudio(data.film.audio || []);
  }

  function layerDetail(layer) {
    switch (layer.type) {
      case "solid":
        return layer.colour || "";
      case "gradient":
        return `${(layer.stops || []).length} stops`;
      case "scrim":
        return "scrim";
      case "still":
        return layer.asset + (layer.camera ? " · camera" : "");
      case "clip":
        return layer.asset + (layer.camera ? " · camera" : "") + (layer.trim ? " · trimmed" : "");
      case "text":
        return truncate(layer.text, 44);
      case "title":
        return truncate(layer.text, 44);
      case "lower-third":
        return truncate(layer.text, 44);
      case "counter":
        return layer.count ? `${layer.count.from} → ${layer.count.to}` : "";
      case "callout":
        return truncate(layer.text, 44);
      case "pull-up":
        return layer.label ? truncate(layer.label, 44) : "region lifted";
      default:
        return "";
    }
  }

  function renderAudio(tracks) {
    const list = $("audio-list");
    list.innerHTML = "";
    if (!tracks.length) return;
    list.appendChild(el("h2", null, "Sound"));
    for (const a of tracks) {
      const row = el("div", "audio-row");
      row.appendChild(el("span", "audio-asset", a.asset));
      row.appendChild(el("span", null, `at ${a.at || 0}s`));
      if (a.duration != null) row.appendChild(el("span", null, `for ${a.duration}s`));
      if (a.gain != null && a.gain !== 1) row.appendChild(el("span", null, `gain ${a.gain}`));
      list.appendChild(row);
    }
  }

  // ---- frame fetching --------------------------------------------------------------

  function frameUrl(t) {
    return `/api/frame?t=${t.toFixed(3)}&v=${state ? state.version : 0}`;
  }

  function renderFrame(t, force) {
    if (!state || !state.film) return;
    if (pendingFrame && !force) return;
    pendingFrame = true;
    const started = performance.now();
    const img = new Image();
    img.onload = () => {
      pendingFrame = false;
      frameImg.src = img.src;
      stage.classList.add("has-frame");
      recordFrameTime(performance.now() - started);
    };
    img.onerror = () => {
      pendingFrame = false;
    };
    img.src = frameUrl(t);
  }

  function recordFrameTime(ms) {
    frameTimes.push(ms);
    if (frameTimes.length > 20) frameTimes.shift();
    if (playing) {
      const avg = frameTimes.reduce((a, b) => a + b, 0) / frameTimes.length;
      const achieved = Math.min(1000 / avg, fps);
      $("fps-readout").textContent = `~${achieved.toFixed(1)} fps (best effort)`;
    } else {
      $("fps-readout").textContent = "";
    }
  }

  // ---- transport ---------------------------------------------------------------------

  function setTime(t, { fromScrub } = {}) {
    currentTime = Math.max(0, Math.min(duration, t));
    updatePlayhead();
    updateTimecode();
    renderFrame(currentTime, fromScrub);
  }

  function togglePlay() {
    playing ? pause() : play();
  }

  function play() {
    if (playing || duration <= 0) return;
    playing = true;
    $("btn-play").textContent = "⏸";
    $("btn-play").classList.add("playing");
    playAnchorWall = performance.now();
    playAnchorTime = currentTime >= duration - 1 / fps ? (loopIn ?? 0) : currentTime;
    frameTimes = [];
    rafId = requestAnimationFrame(tick);
  }

  function pause() {
    playing = false;
    $("btn-play").textContent = "▶";
    $("btn-play").classList.remove("playing");
    $("fps-readout").textContent = "";
    if (rafId) cancelAnimationFrame(rafId);
  }

  function tick() {
    if (!playing) return;
    const elapsed = (performance.now() - playAnchorWall) / 1000;
    let t = playAnchorTime + elapsed;
    const loopActive = $("chk-loop").checked && loopIn != null && loopOut != null && loopOut > loopIn;
    const endBound = loopActive ? loopOut : duration;
    const startBound = loopActive ? loopIn : 0;
    if (t >= endBound) {
      if (loopActive) {
        playAnchorTime = startBound;
        playAnchorWall = performance.now();
        t = startBound;
      } else {
        setTime(duration, { fromScrub: true });
        pause();
        return;
      }
    }
    setTime(t);
    rafId = requestAnimationFrame(tick);
  }

  function step(dir) {
    pause();
    setTime(currentTime + dir / fps, { fromScrub: true });
  }

  // ---- wiring --------------------------------------------------------------------------

  $("btn-play").onclick = togglePlay;
  $("btn-step-back").onclick = () => step(-1);
  $("btn-step-fwd").onclick = () => step(1);
  $("btn-set-in").onclick = () => {
    loopIn = currentTime;
    updateLoopBand();
  };
  $("btn-set-out").onclick = () => {
    loopOut = currentTime;
    updateLoopBand();
  };
  $("btn-clear-loop").onclick = () => {
    loopIn = null;
    loopOut = null;
    $("chk-loop").checked = false;
    updateLoopBand();
  };

  seek.addEventListener("mousedown", pause);
  seek.addEventListener("touchstart", pause);
  seek.addEventListener("input", () => setTime(parseFloat(seek.value), { fromScrub: true }));

  document.addEventListener("keydown", (e) => {
    if (e.target && (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA")) return;
    if (e.code === "Space") {
      e.preventDefault();
      togglePlay();
    } else if (e.key === "," || e.key === "ArrowLeft") {
      step(-1);
    } else if (e.key === "." || e.key === "ArrowRight") {
      step(1);
    }
  });

  // ---- boot -----------------------------------------------------------------------------

  pollState();
  setInterval(pollState, 700);
})();

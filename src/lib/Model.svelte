<script lang="ts">
  import { onMount } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { Box, FileWarning, Pause, Play, Rotate3d, RotateCcw } from "@lucide/svelte";
  import { app } from "./state.svelte";

  let { path }: { path: string } = $props();
  const name = $derived(path.slice(path.lastIndexOf("/") + 1));

  let host: HTMLDivElement;
  let error = $state("");
  let loading = $state(true);
  let stats = $state({ tris: 0, meshes: 0, clips: 0 });
  let spin = $state(false);
  let wire = $state(false);
  let playing = $state(true);
  // Set once the scene exists; the toolbar calls into it.
  let view: { reset(): void; spin(on: boolean): void; wire(on: boolean): void; play(on: boolean): void } | null = null;

  const count = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}k` : String(n));
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : word.endsWith("h") ? "es" : "s"}`;
  const summary = $derived(
    [`${count(stats.tris)} triangles`, plural(stats.meshes, "mesh"), stats.clips && plural(stats.clips, "animation")].filter(Boolean).join(" · "),
  );

  onMount(() => {
    let disposed = false;
    let cleanup = () => {};
    // three.js only loads when a model is opened.
    Promise.all([
      import("three"),
      import("three/addons/loaders/GLTFLoader.js"),
      import("three/addons/controls/OrbitControls.js"),
      import("three/addons/environments/RoomEnvironment.js"),
    ]).then(async ([THREE, { GLTFLoader }, { OrbitControls }, { RoomEnvironment }]) => {
      if (disposed) return;
      const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
      renderer.setPixelRatio(devicePixelRatio);
      renderer.toneMapping = THREE.NeutralToneMapping;
      host.appendChild(renderer.domElement);

      const scene = new THREE.Scene();
      const pmrem = new THREE.PMREMGenerator(renderer);
      scene.environment = pmrem.fromScene(new RoomEnvironment(), 0.04).texture; // soft studio light, no HDR file needed
      const camera = new THREE.PerspectiveCamera(40, 1, 0.01, 1000);
      const controls = new OrbitControls(camera, renderer.domElement);
      controls.enableDamping = true;
      controls.autoRotateSpeed = 1.5;

      const resize = () => {
        const { clientWidth: w, clientHeight: h } = host;
        if (!w || !h) return;
        renderer.setSize(w, h, false);
        camera.aspect = w / h;
        camera.updateProjectionMatrix();
        dirty = true;
      };
      const ro = new ResizeObserver(resize);
      ro.observe(host);

      // Render on demand: orbiting, spinning or an animation playing.
      let dirty = true;
      let mixer: InstanceType<typeof THREE.AnimationMixer> | null = null;
      const clock = new THREE.Clock();
      let frame = 0;
      const loop = () => {
        frame = requestAnimationFrame(loop);
        const dt = clock.getDelta();
        if (mixer && playing) (mixer.update(dt), (dirty = true));
        if (controls.update(dt) || dirty) renderer.render(scene, camera), (dirty = false);
      };
      controls.addEventListener("change", () => (dirty = true));

      cleanup = () => {
        cancelAnimationFrame(frame);
        ro.disconnect();
        controls.dispose();
        scene.traverse((o) => {
          const m = o as InstanceType<typeof THREE.Mesh>;
          if (!m.isMesh) return;
          m.geometry.dispose();
          for (const mat of [m.material].flat()) {
            for (const v of Object.values(mat)) if (v instanceof THREE.Texture) v.dispose();
            mat.dispose();
          }
        });
        scene.environment?.dispose();
        pmrem.dispose();
        renderer.dispose();
        renderer.domElement.remove();
      };

      try {
        const gltf = await new GLTFLoader().loadAsync(convertFileSrc(path));
        if (disposed) return;
        const model = gltf.scene;
        scene.add(model);

        // Center the model and frame it from a three-quarter angle.
        const box = new THREE.Box3().setFromObject(model);
        const size = box.getSize(new THREE.Vector3());
        model.position.sub(box.getCenter(new THREE.Vector3()));
        const radius = Math.max(size.length() / 2, 1e-3);

        let tris = 0, meshes = 0;
        const materials = new Set<{ wireframe?: boolean }>();
        model.traverse((o) => {
          const m = o as InstanceType<typeof THREE.Mesh>;
          if (!m.isMesh) return;
          meshes++;
          const g = m.geometry;
          tris += (g.index ? g.index.count : g.attributes.position.count) / 3;
          for (const mat of [m.material].flat()) materials.add(mat as { wireframe?: boolean });
        });
        stats = { tris: Math.round(tris), meshes, clips: gltf.animations.length };

        if (gltf.animations.length) {
          mixer = new THREE.AnimationMixer(model);
          mixer.clipAction(gltf.animations[0]).play();
        }

        view = {
          reset() {
            // Fit the bounding sphere in the narrower of the two fields of view, from a three-quarter angle.
            const v = THREE.MathUtils.degToRad(camera.fov);
            const h = 2 * Math.atan(Math.tan(v / 2) * camera.aspect);
            const dist = (radius / Math.sin(Math.min(v, h) / 2)) * 0.85; // the box's sphere is loose: frame a bit tighter
            camera.position.set(1, 0.6, 1.3).normalize().multiplyScalar(dist);
            camera.near = dist / 100;
            camera.far = dist * 100;
            controls.maxDistance = dist * 10;
            controls.target.set(0, 0, 0);
            camera.updateProjectionMatrix();
            dirty = true;
          },
          spin: (on) => (controls.autoRotate = on),
          wire(on) {
            for (const mat of materials) mat.wireframe = on;
            dirty = true;
          },
          play: () => (clock.getDelta(), (dirty = true)), // skip the paused time
        };
        resize();
        view.reset();
        loading = false;
        loop();
      } catch (e) {
        if (disposed) return;
        const msg = String(e);
        error = /draco|meshopt|ktx|basis/i.test(msg)
          ? "This model uses a compression extension (Draco, Meshopt or KTX2) the viewer doesn't support."
          : "The file is missing or isn't a valid glTF binary.";
        loading = false;
      }
    });
    return () => ((disposed = true), cleanup());
  });
</script>

<div class="model">
  <div class="canvas" bind:this={host}></div>
  {#if error}
    <div class="empty">
      <FileWarning size={28} strokeWidth={1.25} />
      <span>Can't display <b>{name}</b></span>
      <small>{error}</small>
    </div>
  {:else if loading}
    <div class="empty"><Box size={28} strokeWidth={1.25} /><small>Loading model…</small></div>
  {:else}
    <div class="bar">
      <button class="icon-btn" onclick={() => view?.reset()} title="Reset view"><RotateCcw size={13} strokeWidth={1.5} /></button>
      <button class="icon-btn" class:on={spin} onclick={() => view?.spin((spin = !spin))} title="Turntable"><Rotate3d size={13} strokeWidth={1.5} /></button>
      <button class="icon-btn" class:on={wire} onclick={() => view?.wire((wire = !wire))} title="Wireframe"><Box size={13} strokeWidth={1.5} /></button>
      {#if stats.clips}
        <button class="icon-btn" onclick={() => view?.play((playing = !playing))} title={playing ? "Pause animation" : "Play animation"}>
          {#if playing}<Pause size={13} strokeWidth={1.5} />{:else}<Play size={13} strokeWidth={1.5} />{/if}
        </button>
      {/if}
      <span class="stats">{summary}</span>
    </div>
  {/if}
</div>

<style>
  .model {
    position: absolute;
    inset: 0;
    background: radial-gradient(ellipse at 50% 40%, var(--surface) 0%, color-mix(in oklch, var(--surface), var(--muted-foreground) 14%) 100%);
  }
  .canvas {
    position: absolute;
    inset: 0;
  }
  .canvas :global(canvas) {
    display: block;
    width: 100%;
    height: 100%;
    cursor: grab;
  }
  .canvas :global(canvas:active) {
    cursor: grabbing;
  }
  .bar {
    position: absolute;
    bottom: 14px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background: var(--popover);
    box-shadow: var(--shadow-raised);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--muted-foreground);
    white-space: nowrap;
  }
  .icon-btn.on {
    color: var(--foreground);
    background: var(--accent);
  }
  .stats {
    padding: 0 8px 0 6px;
    border-left: 1px solid var(--border);
  }
  .empty {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    color: var(--muted-foreground);
    font-size: 13px;
    pointer-events: none;
  }
  .empty b {
    color: var(--foreground);
    font-weight: 500;
  }
</style>

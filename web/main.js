import { Renderer, VERTEX_FLOATS } from './render.js';
import { startApp } from './ui.js';

const canvas = document.getElementById('view');
const gl = new Renderer(canvas);

const renderer = {
  resize: () => gl.resize(),
  draw(river, memory, { bounds, local, clock }) {
    river.sync(local.oxbowShrink, local.ageSpan);
    // Views must be rebuilt after every step: memory growth detaches them.
    const buf = memory.buffer;
    const nOx = river.oxbow_count();
    const views = {
      channel: new Float32Array(buf, river.channel_ptr(), river.channel_vertices() * VERTEX_FLOATS),
      oxbows: new Float32Array(buf, river.oxbows_ptr(), river.oxbow_vertices() * VERTEX_FLOATS),
      oxbowOffsets: new Uint32Array(buf, river.oxbow_offsets_ptr(), nOx + 1),
      oxbowAges: new Float32Array(buf, river.oxbow_ages_ptr(), nOx),
      ages: new Uint8Array(buf, river.ages_ptr(), river.age_cols() * river.age_rows()),
      ageSize: [river.age_cols(), river.age_rows()],
      erod: new Uint8Array(buf, river.erod_ptr(), river.erod_cols() * river.erod_rows()),
      erodSize: [river.erod_cols(), river.erod_rows()],
    };
    gl.draw(views, {
      bounds,
      width: river.get_param('width'),
      clock,
      flow: local.flow,
      depthK: local.depthK,
      showAge: local.showAge,
      showErod: local.showErod,
    });
  },
};

startApp({
  view: canvas,
  renderer,
  display: [
    { key: 'depthK', label: 'Bend depth asymmetry', min: 0, max: 4, step: 0.1, local: 1.5 },
    { key: 'flow', label: 'Flow streak speed', min: 0, max: 3, step: 0.1, local: 1 },
    { key: 'oxbowShrink', label: 'Oxbow narrowing', min: 0, max: 0.95, step: 0.05, local: 0.6 },
    { key: 'ageSpan', label: 'Floodplain age span', min: 50, max: 3000, step: 50, local: 800 },
    { key: 'showAge', label: 'Show floodplain age', toggle: true, local: true },
    { key: 'showErod', label: 'Show erodibility', toggle: true, local: false },
  ],
});

// Runs the force simulation off the UI thread so large vaults never drop frames when opening Graph.
import { layout, ring } from './graph-layout.mjs';

self.onmessage = ({ data }) => {
  const { id, linked, loose, links } = data;
  const nodes = linked.map(key => ({ key }));
  layout(nodes, links);
  let all = nodes;
  if (loose?.length) {
    const outer = loose.map(key => ({ key }));
    ring(outer, nodes);
    all = nodes.concat(outer);
  }
  const coords = new Float64Array(all.length * 2);
  for (let i = 0; i < all.length; i++) {
    coords[i * 2] = all[i].x;
    coords[i * 2 + 1] = all[i].y;
  }
  self.postMessage({ id, coords }, [coords.buffer]);
};

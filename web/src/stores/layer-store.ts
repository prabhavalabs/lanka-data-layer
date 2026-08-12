import { create } from "zustand";

import { LAYER_REGISTRY } from "@/components/map/layer-registry";

interface LayerStore {
  /** Layer id -> visible. Seeded from LAYER_REGISTRY's defaultVisible. */
  visibility: Record<string, boolean>;
  toggleLayer: (id: string) => void;
  setLayerVisible: (id: string, visible: boolean) => void;
}

function initialVisibility(): Record<string, boolean> {
  return Object.fromEntries(LAYER_REGISTRY.map((layer) => [layer.id, layer.defaultVisible]));
}

export const useLayerStore = create<LayerStore>((set) => ({
  visibility: initialVisibility(),
  toggleLayer: (id) =>
    set((state) => ({
      visibility: { ...state.visibility, [id]: !state.visibility[id] },
    })),
  setLayerVisible: (id, visible) =>
    set((state) => ({
      visibility: { ...state.visibility, [id]: visible },
    })),
}));

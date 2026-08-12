import { MapView } from "@/components/map/map-view";
import { useMapUrlSync } from "@/hooks/use-map-url-sync";

export function MapPage() {
  useMapUrlSync();

  return (
    <div className="absolute inset-0">
      <MapView />
    </div>
  );
}

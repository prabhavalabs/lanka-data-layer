import * as React from "react";

import { ApiError, apiGetPayload } from "@/lib/api";
import type { Dataset } from "@/lib/dataset";

export type DatasetsState =
  | { status: "loading" }
  | { status: "error"; error: ApiError }
  | { status: "success"; data: Dataset[] };

/** Fetches `/v1/datasets` and tracks loading/error/success state. */
export function useDatasets(): DatasetsState {
  const [state, setState] = React.useState<DatasetsState>({ status: "loading" });

  React.useEffect(() => {
    const controller = new AbortController();
    setState({ status: "loading" });

    apiGetPayload<Dataset[]>("/datasets", { signal: controller.signal })
      .then((data) => setState({ status: "success", data }))
      .catch((err: unknown) => {
        if (err instanceof DOMException && err.name === "AbortError") return;
        setState({
          status: "error",
          error: err instanceof ApiError ? err : new ApiError(String(err), 0),
        });
      });

    return () => controller.abort();
  }, []);

  return state;
}

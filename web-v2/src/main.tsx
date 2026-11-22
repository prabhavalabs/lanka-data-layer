import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router'
import { onEngineRevisionChange } from './api/engine-revision'
import { App } from './App'
import './index.css'

const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: 5 * 60 * 1000, refetchOnWindowFocus: false, retry: 1 } },
})
// analysis ids include the engine's revision: results (and the layers drawn from them) of an earlier revision are dropped
onEngineRevisionChange(() => void queryClient.invalidateQueries({ predicate: (q) => q.queryKey.includes('analysis') }))

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
)

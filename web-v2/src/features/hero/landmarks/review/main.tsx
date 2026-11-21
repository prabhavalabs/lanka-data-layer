// Dev-only entry for the landmark review harness. It is its own HTML page, so it is
// never part of the production build (Vite only bundles the root index.html).
//   /src/features/hero/landmarks/review/index.html            contact sheet, all nine
//   …?m=sigiriya                                              one model, close up
//   &theme=light|dark  &rot=<deg from the hero view>  &elev=<deg>  &spin=1  &zoom=<×>
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@/index.css'
import { Review } from './Review'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Review />
  </StrictMode>,
)

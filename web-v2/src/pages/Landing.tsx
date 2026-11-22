import { Hero } from '@/features/hero/Hero'
import { AreaTeaser, CatalogueTeaser, Closing, Honesty, Interfaces, Pillars, ProvenanceShowcase, StatsStrip, Themes } from '@/features/landing/sections'

export default function Landing() {
  return (
    <>
      <Hero />
      <StatsStrip />
      <Pillars />
      <ProvenanceShowcase />
      <Interfaces />
      <Themes />
      <AreaTeaser />
      <CatalogueTeaser />
      <Honesty />
      <Closing />
    </>
  )
}

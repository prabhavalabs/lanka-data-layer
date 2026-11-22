// One material for every landmark mesh (static bodies and animated parts alike),
// so the whole layer is a single program and the theme cross-fade costs one
// uniform write per frame.
import * as THREE from 'three'

/** written by <Landmarks> once per frame; shared by every mesh using the material */
export const landmarkUniforms = {
  /** 0 = night palette, 1 = day palette */
  uDay: { value: 0 },
  /** emissive strength: theme.landmark.glow, eased */
  uGlow: { value: 1 },
  uEmissive: { value: new THREE.Color('#ffd49a') },
  uTime: { value: 0 },
}

let shared: THREE.MeshStandardMaterial | null = null

export function landmarkMaterial() {
  if (shared) return shared
  const m = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.82, metalness: 0 })
  m.onBeforeCompile = (shader) => {
    Object.assign(shader.uniforms, landmarkUniforms)
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nattribute vec3 colorDay;\nattribute vec3 fx;\nuniform float uDay;\nvarying vec3 vFx;')
      .replace('#include <color_vertex>', '#include <color_vertex>\nvColor.rgb = mix(color.rgb, colorDay, uDay);\nvFx = fx;')
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', '#include <common>\nuniform float uGlow;\nuniform float uTime;\nuniform vec3 uEmissive;\nvarying vec3 vFx;')
      .replace('#include <roughnessmap_fragment>', '#include <roughnessmap_fragment>\nroughnessFactor = mix(roughnessFactor, 0.16, vFx.y);')
      .replace(
        '#include <emissivemap_fragment>',
        `#include <emissivemap_fragment>
         {
           float g = abs(vFx.x);
           float tw = 1.0 + step(0.001, vFx.z) * 0.45 * sin(uTime * 2.4 + vFx.z * 43.0);
           // warm theme light for lamps and windows; negative glow keeps the surface's own hue
           vec3 hue = vFx.x < 0.0 ? diffuseColor.rgb * 1.6 : uEmissive;
           totalEmissiveRadiance += hue * g * uGlow * tw;
         }`,
      )
  }
  m.customProgramCacheKey = () => 'ldl-landmark'
  shared = m
  return m
}

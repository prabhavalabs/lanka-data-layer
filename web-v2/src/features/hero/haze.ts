import type { Camera } from 'three'

/**
 * Distance haze is measured against how far the camera stands from the island, so the
 * island sits in the same clear water on a phone (camera further back) as on a desktop.
 * The sea and everything drawn on it must use the same rate, or lanes would outlast the water.
 */
export const hazeRate = (camera: Camera) => 0.285 / Math.max(1, camera.position.length())

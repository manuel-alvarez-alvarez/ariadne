/**
 * A dashed straight edge for sigma.js, which ships solid lines only.
 *
 * It is sigma's own rectangle edge (`EdgeRectangleProgram`, the default
 * `line`) with one value more: how far along the edge a fragment is, in
 * half-widths of the edge, so the dashes keep their length on screen at any
 * zoom. The fragment shader drops the gaps. Picking mode keeps the whole
 * line, so a click in a gap still hits the edge.
 */

import { EdgeRectangleProgram } from "sigma/rendering"

// language=GLSL
const VERTEX_SHADER_SOURCE = /*glsl*/ `
attribute vec4 a_id;
attribute vec4 a_color;
attribute vec2 a_normal;
attribute float a_normalCoef;
attribute vec2 a_positionStart;
attribute vec2 a_positionEnd;
attribute float a_positionCoef;

uniform mat3 u_matrix;
uniform float u_sizeRatio;
uniform float u_zoomRatio;
uniform float u_pixelRatio;
uniform float u_correctionRatio;
uniform float u_minEdgeThickness;
uniform float u_feather;

varying vec4 v_color;
varying vec2 v_normal;
varying float v_thickness;
varying float v_feather;
varying float v_along;

const float bias = 255.0 / 254.0;

void main() {
  float minThickness = u_minEdgeThickness;

  vec2 normal = a_normal * a_normalCoef;
  vec2 position = a_positionStart * (1.0 - a_positionCoef) + a_positionEnd * a_positionCoef;

  float normalLength = length(normal);
  vec2 unitNormal = normal / normalLength;

  float pixelsThickness = max(normalLength, minThickness * u_sizeRatio);
  float webGLThickness = pixelsThickness * u_correctionRatio / u_sizeRatio;

  gl_Position = vec4((u_matrix * vec3(position + unitNormal * webGLThickness, 1)).xy, 0, 1);

  v_thickness = webGLThickness / u_zoomRatio;
  v_normal = unitNormal;
  v_feather = u_feather * u_correctionRatio / u_zoomRatio / u_pixelRatio * 2.0;
  v_along = a_positionCoef * length(a_positionEnd - a_positionStart) / webGLThickness;

  #ifdef PICKING_MODE
  v_color = a_id;
  #else
  v_color = a_color;
  #endif

  v_color.a *= bias;
}
`

// language=GLSL
const FRAGMENT_SHADER_SOURCE = /*glsl*/ `
precision mediump float;

varying vec4 v_color;
varying vec2 v_normal;
varying float v_thickness;
varying float v_feather;
varying float v_along;

const vec4 transparent = vec4(0.0, 0.0, 0.0, 0.0);
const float dash = 8.0;
const float gap = 6.0;

void main(void) {
  #ifdef PICKING_MODE
  gl_FragColor = v_color;
  #else
  if (mod(v_along, dash + gap) > dash) discard;

  float dist = length(v_normal) * v_thickness;
  float t = smoothstep(v_thickness - v_feather, v_thickness, dist);
  gl_FragColor = mix(v_color, transparent, t);
  #endif
}
`

export class DashedEdgeProgram extends EdgeRectangleProgram {
  override getDefinition() {
    return { ...super.getDefinition(), VERTEX_SHADER_SOURCE, FRAGMENT_SHADER_SOURCE }
  }
}

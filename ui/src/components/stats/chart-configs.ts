/**
 * Every chart's own series config, in one file rather than one per section,
 * so "failed is danger" and "a neutral count is active" are each written once
 * and every section that draws that meaning imports the same constant — a
 * wrong meaning colour is then a diff here, not a colour that quietly drifts
 * from the one two sections down. Each family adds the configs its charts
 * draw, built off `STATUS_COLORS`.
 */

export {}

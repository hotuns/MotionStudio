import type {Gyro, StickResponsePoint} from './types';

export type CurveRow = [travel: string, speed: string, exponent: string];
export const linearStickResponse: StickResponsePoint[] = [[0, 0], [1, 1]];
export const maxStickResponsePoints = 256;
type Parsed = {points: StickResponsePoint[]; error?: never} | {error: string; points?: never};

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));
const snap = (value: number) => Math.round(value * 1000) / 1000; // 0.1% on the plot.

/** Nearest drag increment strictly inside a neighbour, at the engine's f32 precision. */
function insideTick(edge: number, direction: 1 | -1) {
  const tick = Math.round(edge * 1000);
  return (Math.sign(Math.fround(tick / 1000) - Math.fround(edge)) === direction ? tick : tick + direction) / 1000;
}

export function moveCurvePoint(points: StickResponsePoint[], index: number, travel: number, speed: number) {
  const prev = points[index - 1], point = points[index], next = points[index + 1];
  if (!prev || !next) return points;
  const min = insideTick(prev[0], 1), max = insideTick(next[0], -1);
  // Imported points can be closer together than a pointer increment. Keep their X
  // when no increment fits, and keep the exact coordinate on an untouched axis.
  const x = travel === point[0] || min > max ? point[0] : clamp(snap(travel), min, max);
  const y = speed === point[1] ? point[1] : clamp(snap(speed), prev[1], next[1]);
  if (x === point[0] && y === point[1]) return points;
  const moved: StickResponsePoint = [...point];
  moved[0] = x;
  moved[1] = y;
  return points.map((p, i) => i === index ? moved : p);
}

export function insertCurvePoint(points: StickResponsePoint[], travel: number, speed: number) {
  const x = clamp(snap(travel), 0, 1);
  const index = points.findIndex(p => Math.fround(p[0]) >= Math.fround(x));
  if (index < 0 || Math.fround(points[index][0]) === Math.fround(x)) return {points, index};
  if (points.length >= maxStickResponsePoints) return {points, index: -1};
  const y = clamp(snap(speed), points[index - 1][1], points[index][1]);
  const point: StickResponsePoint = [x, y];
  return {points: [...points.slice(0, index), point, ...points.slice(index)], index};
}

export function selectStickResponse(gyro: Gyro, piecewise: boolean) {
  const curve = {...gyro.right_stick_curve, enabled: piecewise};
  if (piecewise && !curve.points) {
    const inner = gyro.jsm_response?.right_undeadzone_inner ?? 0;
    const outer = gyro.jsm_response?.right_undeadzone_outer ?? 0;
    const power = gyro.jsm_response?.right_unpower || 1;
    // Seed the first point table with the user's existing in-game response.
    // With the default linear settings, leave points available for inheritance.
    if (inner || outer || power !== 1) {
      const points: StickResponsePoint[] = [];
      if (inner > 0) points.push([0, 0]);
      points.push(power === 1 ? [inner, 0] : [inner, 0, power], [1 - outer, 1]);
      if (outer > 0) points.push([1, 1]);
      curve.points = points;
    }
  }
  gyro.right_stick_curve = curve;
}

export function curveRows(points: StickResponsePoint[]): CurveRow[] {
  const percent = (v: number) => String(Number((v * 100).toPrecision(9)));
  return points.map(([travel, speed, exponent = 1]) =>
    [percent(travel), percent(speed), exponent === 1 ? '' : String(exponent)]);
}

function numeric(cell: string): number {
  return /^[+-]?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?$/i.test(cell) ? Number(cell) : NaN;
}

export function parseCurveRows(rows: CurveRow[], scale = 100): Parsed {
  if (rows.length < 2 || rows.length > maxStickResponsePoints) {
    return {error: '分段曲线需包含 2 到 256 个点。'};
  }
  const points: StickResponsePoint[] = [];
  for (const [i, row] of rows.entries()) {
    const coordinate = (cell: string) => numeric(cell.trim().replace(/[%％]$/, '').trim()) / scale;
    const travel = coordinate(row[0]), speed = coordinate(row[1]);
    const power = row[2].trim() === '' ? 1 : numeric(row[2].trim());
    if (!(travel >= 0 && travel <= 1 && speed >= 0 && speed <= 1)) {
      return {error: `第 ${i + 1} 行的推杆量和游戏转速必须在 0% 到 100% 之间。`};
    }
    if (!Number.isFinite(Math.fround(power)) || power <= 0 || !Number.isFinite(Math.fround(1 / Math.fround(power)))) {
      return {error: `第 ${i + 1} 行的段指数必须是有限正数。`};
    }
    const prev = points.at(-1);
    if (prev && (Math.fround(travel) <= Math.fround(prev[0]) || Math.fround(speed) < Math.fround(prev[1]))) {
      return {error: `第 ${i + 1} 行的推杆量必须递增，游戏转速可以持平或增加。`};
    }
    points.push(power === 1 ? [travel, speed] : [travel, speed, power]);
  }
  const first = points[0], last = points[points.length - 1];
  if (first[0] !== 0 || first[1] !== 0 || last[0] !== 1 || last[1] !== 1) {
    return {error: '分段曲线的起点和终点必须分别是 (0%, 0%) 和 (100%, 100%)。'};
  }
  if (last[2] !== undefined) return {error: '终点没有下一段，段指数请留空或填 1。'};
  return {points};
}

export function parseCurveText(text: string): Parsed {
  const rows: CurveRow[] = [];
  for (const line of text.split(/[\r\n;；]+/).map(line => line.trim()).filter(Boolean)) {
    const cells = line.split(/[,，]/.test(line) ? /[,，]/ : line.includes('\t') ? /\t/ : /\s+/);
    if (cells.length < 2 || cells.length > 3) {
      return {error: `第 ${rows.length + 1} 行需要两列数值，可选第三列段指数。`};
    }
    rows.push([cells[0], cells[1], cells[2] ?? '']);
  }
  // Required endpoints disambiguate normalized source tables from percentages.
  const end = rows.at(-1);
  const normalized = end && !/[%％]/.test(text) && numeric(end[0].trim()) === 1 && numeric(end[1].trim()) === 1;
  return parseCurveRows(rows, normalized ? 1 : 100);
}

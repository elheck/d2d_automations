// A small SVG line chart (no chart library: nothing extra to load or audit,
// and nothing that needs inline styles, which the CSP blocks). Series share
// one y-axis starting at 0; missing values leave gaps. Hovering or tapping
// shows the values of the nearest date; a hidden table carries the same
// data for screen readers.

import { useEffect, useId, useMemo, useRef, useState, type PointerEvent } from "react";

export interface ChartSeries {
  id: string;
  label: string;
  /** CSS class with the series colour (e.g. `chart-series--ev`). */
  className: string;
  values: (number | null)[];
}

interface Props {
  dates: string[];
  series: ChartSeries[];
  /** Text for screen readers, e.g. "EV and sealed price per week". */
  label: string;
  format: (value: number) => string;
  /** Axis labels (default: `format`), e.g. without cents. */
  tickFormat?: (value: number) => string;
  height?: number;
}

const MARGIN = { top: 12, right: 12, bottom: 28, left: 64 };
const FALLBACK_WIDTH = 720;

/** A "nice" step (1, 2, 2.5 or 5 × 10^n) for about `count` ticks up to `max`. */
export function niceStep(max: number, count = 4): number {
  if (!(max > 0)) return 1;
  const raw = max / count;
  const power = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * power).find((s) => s >= raw);
  return step ?? 10 * power;
}

/** SVG path of a series, broken at missing values. */
export function linePath(values: (number | null)[], x: (i: number) => number, y: (v: number) => number): string {
  let path = "";
  let drawing = false;
  values.forEach((value, i) => {
    if (value === null) {
      drawing = false;
      return;
    }
    path += `${drawing ? "L" : "M"}${x(i).toFixed(1)} ${y(value).toFixed(1)} `;
    drawing = true;
  });
  return path.trim();
}

/** `2026-03-02` → `Mar 2` (English, short). */
export function shortDate(iso: string): string {
  const date = new Date(`${iso}T00:00:00Z`);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
}

/**
 * Which dates get an x-axis label: about one per `every`, always the first
 * and the last, and none so close to the last that they would overlap it.
 */
export function labelledDates(count: number, every: number): number[] {
  if (count === 0) return [];
  const last = count - 1;
  const out: number[] = [];
  for (let i = 0; i < last; i += every) {
    // The last label is right-aligned, so it needs a whole interval of room.
    if (last - i >= every || i === 0) out.push(i);
  }
  if (!out.includes(last)) out.push(last);
  return out;
}

export function LineChart({ dates, series, label, format, tickFormat = format, height = 240 }: Props) {
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(FALLBACK_WIDTH);
  const [hover, setHover] = useState<number | null>(null);
  const tableId = useId();

  useEffect(() => {
    const element = box.current;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      const measured = entries[0]?.contentRect.width;
      if (measured && measured > 0) setWidth(Math.round(measured));
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const plotWidth = Math.max(width - MARGIN.left - MARGIN.right, 10);
  const plotHeight = height - MARGIN.top - MARGIN.bottom;
  const max = Math.max(0, ...series.flatMap((s) => s.values.filter((v): v is number => v !== null)));
  const step = niceStep(max);
  const top = Math.max(step, Math.ceil(max / step) * step);
  const ticks = useMemo(() => {
    const out: number[] = [];
    for (let v = 0; v <= top + step / 2; v += step) out.push(v);
    return out;
  }, [top, step]);
  const x = (i: number) => MARGIN.left + (dates.length <= 1 ? plotWidth / 2 : (i / (dates.length - 1)) * plotWidth);
  const y = (v: number) => MARGIN.top + plotHeight - (v / top) * plotHeight;
  const labelEvery = Math.max(1, Math.ceil(dates.length / Math.max(2, Math.floor(plotWidth / 90))));

  function pick(event: PointerEvent<SVGRectElement>) {
    if (dates.length === 0) return;
    const rect = event.currentTarget.getBoundingClientRect();
    const position = (event.clientX - rect.left) / Math.max(rect.width, 1);
    setHover(Math.min(dates.length - 1, Math.max(0, Math.round(position * (dates.length - 1)))));
  }

  const shown = hover ?? dates.length - 1;
  return (
    <figure className="chart" ref={box}>
      <svg
        className="chart__svg"
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label={label}
        aria-describedby={tableId}
      >
        {ticks.map((t) => (
          <g key={t}>
            <line className="chart__grid" x1={MARGIN.left} x2={MARGIN.left + plotWidth} y1={y(t)} y2={y(t)} />
            <text className="chart__tick" x={MARGIN.left - 8} y={y(t)} textAnchor="end" dominantBaseline="middle">
              {tickFormat(t)}
            </text>
          </g>
        ))}
        {labelledDates(dates.length, labelEvery).map((i) => (
          <text
            key={dates[i]}
            className="chart__tick"
            x={x(i)}
            y={height - 8}
            textAnchor={
              dates.length > 1 && i === dates.length - 1 ? "end" : i === 0 && dates.length > 1 ? "start" : "middle"
            }
          >
            {shortDate(dates[i] ?? "")}
          </text>
        ))}
        {series.map((s) => (
          <g key={s.id} className={s.className}>
            <path className="chart__line" d={linePath(s.values, x, y)} />
            {s.values.map((v, i) => {
              const alone = v !== null && (s.values[i - 1] ?? null) === null && (s.values[i + 1] ?? null) === null;
              return alone || (hover === i && v !== null) ? (
                <circle key={i} className="chart__dot" cx={x(i)} cy={y(v ?? 0)} r={3.5} />
              ) : null;
            })}
          </g>
        ))}
        {hover !== null && (
          <line className="chart__cursor" x1={x(hover)} x2={x(hover)} y1={MARGIN.top} y2={MARGIN.top + plotHeight} />
        )}
        <rect
          className="chart__hit"
          x={MARGIN.left}
          y={MARGIN.top}
          width={plotWidth}
          height={plotHeight}
          onPointerMove={pick}
          onPointerDown={pick}
          onPointerLeave={() => setHover(null)}
        />
      </svg>
      <figcaption className="chart__legend">
        {dates.length > 0 && <span className="chart__date">{dates[shown]}</span>}
        {series.map((s) => {
          const value = s.values[shown] ?? null;
          return (
            <span key={s.id} className={`chart__key ${s.className}`}>
              <span className="chart__swatch" aria-hidden="true" />
              {s.label}: {value === null ? "—" : format(value)}
            </span>
          );
        })}
      </figcaption>
      <table className="visually-hidden" id={tableId}>
        <caption>{label}</caption>
        <thead>
          <tr>
            <th scope="col">Date</th>
            {series.map((s) => (
              <th key={s.id} scope="col">
                {s.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {dates.map((d, i) => (
            <tr key={d}>
              <th scope="row">{d}</th>
              {series.map((s) => {
                const value = s.values[i] ?? null;
                return <td key={s.id}>{value === null ? "—" : format(value)}</td>;
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </figure>
  );
}

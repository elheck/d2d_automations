import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDebounced } from "./useDebounced";

describe("useDebounced", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("follows the value only after it settled", () => {
    const { result, rerender } = renderHook(({ value }) => useDebounced(value, 300), {
      initialProps: { value: { a: 1 } },
    });
    expect(result.current).toEqual({ a: 1 });
    rerender({ value: { a: 2 } });
    act(() => vi.advanceTimersByTime(200));
    rerender({ value: { a: 3 } });
    act(() => vi.advanceTimersByTime(200));
    expect(result.current).toEqual({ a: 1 });
    act(() => vi.advanceTimersByTime(100));
    expect(result.current).toEqual({ a: 3 });
  });

  it("keeps the same object for equal content", () => {
    const { result, rerender } = renderHook(({ value }) => useDebounced(value, 300), {
      initialProps: { value: { a: 1 } as { a: number } | null },
    });
    const first = result.current;
    rerender({ value: { a: 1 } });
    act(() => vi.advanceTimersByTime(500));
    expect(result.current).toBe(first);
    rerender({ value: null });
    act(() => vi.advanceTimersByTime(300));
    expect(result.current).toBeNull();
  });
});

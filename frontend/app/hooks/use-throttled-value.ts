import * as React from "react";

export function useThrottledValue<T>(value: T, intervalMs: number, enabled = true): T {
  const [displayValue, setDisplayValue] = React.useState(value);
  const latestValueRef = React.useRef(value);
  const timerRef = React.useRef<number | null>(null);

  React.useEffect(() => {
    latestValueRef.current = value;

    if (!enabled || typeof window === "undefined") {
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      setDisplayValue(value);
      return;
    }

    if (timerRef.current !== null) return;
    timerRef.current = window.setTimeout(() => {
      timerRef.current = null;
      setDisplayValue(latestValueRef.current);
    }, intervalMs);
  }, [enabled, intervalMs, value]);

  React.useEffect(
    () => () => {
      if (timerRef.current !== null && typeof window !== "undefined") {
        window.clearTimeout(timerRef.current);
      }
    },
    [],
  );

  return enabled ? displayValue : value;
}

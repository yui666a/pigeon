const LOCALE = "ja-JP";

/** Short date for list items: "4/15" */
export function formatShortDate(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleDateString(LOCALE, { month: "numeric", day: "numeric" });
}

/** Full date-time for mail headers: "2026/4/15 10:30:00" */
export function formatFullDate(dateStr: string): string {
  return new Date(dateStr).toLocaleString(LOCALE);
}

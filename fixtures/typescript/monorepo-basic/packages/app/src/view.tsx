import { Circle } from "@fixture/core";

export function Badge({ label }: { label: string }) {
  return <span title={label}>{label}</span>;
}

export const CircleBadge = () => <Badge label={new Circle(2).name} />;

import type { ElementType, ReactNode } from "react";

/**
 * Renders transcript text with per-block direction resolution so mixed
 * Hebrew/English content lays out correctly: `dir="auto"` picks the direction
 * from the first strong character and `unicode-bidi: plaintext` keeps each
 * paragraph independent.
 */
export function DirectionalText({
  as: Tag = "p",
  className = "",
  children,
}: {
  as?: ElementType;
  className?: string;
  children: ReactNode;
}) {
  return (
    <Tag dir="auto" style={{ unicodeBidi: "plaintext" }} className={className}>
      {children}
    </Tag>
  );
}

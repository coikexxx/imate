/** 人设固定的四段，顺序即书写顺序。必须与后端 persona.rs 的 SECTIONS 一致。 */
export const PERSONA_SECTIONS = ["定位", "职责", "边界", "工作风格"] as const;

export type PersonaSection = (typeof PERSONA_SECTIONS)[number];
export type PersonaParts = Record<PersonaSection, string>;

export function emptyPersona(): PersonaParts {
  return { 定位: "", 职责: "", 边界: "", 工作风格: "" };
}

/** 把人设 Markdown 拆成四段正文（不含标题，首尾去空白）。 */
export function parsePersona(markdown: string): PersonaParts {
  const lines: Record<PersonaSection, string[]> = { 定位: [], 职责: [], 边界: [], 工作风格: [] };
  let current: PersonaSection | null = null;
  for (const line of markdown.split(/\r?\n/)) {
    const heading = PERSONA_SECTIONS.find((section) => line.trim() === `## ${section}`);
    if (heading) {
      current = heading;
      continue;
    }
    if (current) lines[current].push(line);
  }
  const parts = emptyPersona();
  for (const section of PERSONA_SECTIONS) {
    parts[section] = lines[section].join("\n").trim();
  }
  return parts;
}

/** 把四段正文拼回人设 Markdown。 */
export function composePersona(parts: PersonaParts): string {
  return (
    PERSONA_SECTIONS.map((section) => `## ${section}\n\n${parts[section].trim()}`).join("\n\n") +
    "\n"
  );
}

export function isPersonaComplete(parts: PersonaParts): boolean {
  return PERSONA_SECTIONS.every((section) => parts[section].trim() !== "");
}

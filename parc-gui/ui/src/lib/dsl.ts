export interface DslToken {
  type: "filter" | "hashtag" | "phrase" | "word";
  field?: string;
  value: string;
  start: number;
  end: number;
}

const FILTER_FIELDS = [
  "type", "status", "priority", "tag", "due", "created", "updated",
  "by", "has", "linked", "is",
];

export function tokenize(input: string): DslToken[] {
  const tokens: DslToken[] = [];
  let i = 0;

  while (i < input.length) {
    // Skip whitespace
    if (input[i] === " ") { i++; continue; }

    const start = i;

    // Quoted phrase
    if (input[i] === '"') {
      i++;
      let phrase = "";
      while (i < input.length && input[i] !== '"') {
        phrase += input[i];
        i++;
      }
      if (i < input.length) i++; // skip closing quote
      tokens.push({ type: "phrase", value: phrase, start, end: i });
      continue;
    }

    // Hashtag
    if (input[i] === "#") {
      i++;
      let tag = "";
      while (i < input.length && input[i] !== " ") {
        tag += input[i];
        i++;
      }
      tokens.push({ type: "hashtag", value: tag, start, end: i });
      continue;
    }

    // Word or filter
    let word = "";
    while (i < input.length && input[i] !== " ") {
      word += input[i];
      i++;
    }

    const colonIdx = word.indexOf(":");
    if (colonIdx > 0) {
      const field = word.slice(0, colonIdx);
      const value = word.slice(colonIdx + 1);
      if (FILTER_FIELDS.includes(field)) {
        tokens.push({ type: "filter", field, value, start, end: i });
        continue;
      }
    }

    tokens.push({ type: "word", value: word, start, end: i });
  }

  return tokens;
}

export function getSuggestions(
  input: string,
  cursorPos: number
): string[] {
  // Find which token the cursor is in
  const tokens = tokenize(input);
  const currentToken = tokens.find(
    (t) => cursorPos >= t.start && cursorPos <= t.end
  );

  if (!currentToken) {
    // Suggest filter fields
    return FILTER_FIELDS.map((f) => `${f}:`);
  }

  if (currentToken.type === "filter" || currentToken.type === "word") {
    const text = currentToken.value || input.slice(currentToken.start, cursorPos);
    const colonIdx = text.indexOf(":");
    if (colonIdx < 0) {
      // Suggest matching filter fields
      return FILTER_FIELDS
        .filter((f) => f.startsWith(text.toLowerCase()))
        .map((f) => `${f}:`);
    }

    const field = text.slice(0, colonIdx);
    const val = text.slice(colonIdx + 1).toLowerCase();

    const valueSuggestions: Record<string, string[]> = {
      type: ["note", "todo", "decision", "risk", "idea"],
      status: ["open", "active", "done", "cancelled", "accepted", "rejected", "mitigated"],
      priority: ["critical", "high", "medium", "low", "none"],
      has: ["attachments", "links", "due"],
      is: ["archived", "all"],
      due: ["today", "this-week", "this-month", "overdue"],
      created: ["today", "this-week", "this-month", "7-days-ago", "30-days-ago"],
      updated: ["today", "this-week", "this-month", "7-days-ago", "30-days-ago"],
    };

    if (valueSuggestions[field]) {
      return valueSuggestions[field]
        .filter((v) => v.startsWith(val))
        .map((v) => `${field}:${v}`);
    }
  }

  return [];
}

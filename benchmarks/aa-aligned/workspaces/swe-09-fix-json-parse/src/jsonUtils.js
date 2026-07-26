export function safeParse(text) {
  try {
    const data = JSON.parse(text);
    return { error: null, data };
  } catch (e) {
    return { error: "invalid json", data: null };
  }
}
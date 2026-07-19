export function shouldAcceptEventSequence(previous: number, next: unknown): boolean {
  const sequence = Number(next ?? 0);
  if (!Number.isFinite(sequence) || sequence <= 0) {
    return true;
  }
  return sequence > previous;
}

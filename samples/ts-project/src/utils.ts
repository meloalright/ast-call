export function formatName(first: string, last: string): string {
  return `${first} ${last}`;
}

export function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

export const greet = (name: string): string => {
  return `Hello, ${capitalize(name)}!`;
};

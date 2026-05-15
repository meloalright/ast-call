export interface Speaker {
  speak(): string;
  greet(name: string): string;
}

export class Dog implements Speaker {
  private name: string;

  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return "Woof!";
  }

  greet(name: string): string {
    return `${this.name} barks at ${name}`;
  }
}

export class Cat implements Speaker {
  speak(): string {
    return "Meow!";
  }

  greet(name: string): string {
    return `Cat meows at ${name}`;
  }
}

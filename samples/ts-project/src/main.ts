import { formatName, greet } from './utils';
import { Dog, Cat } from './animal';
import type { Speaker } from './animal';

function makeSpeaker(kind: string): Speaker {
  if (kind === 'dog') {
    return new Dog('Rex');
  }
  return new Cat();
}

function main() {
  const name = formatName('John', 'Doe');
  console.log(greet(name));

  const speaker = makeSpeaker('dog');
  console.log(speaker.speak());
  console.log(speaker.greet('World'));
}

main();

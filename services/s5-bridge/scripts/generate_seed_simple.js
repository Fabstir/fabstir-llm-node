// Generate a valid 15-word S5.js seed phrase using crypto primitives only

// Polyfill browser APIs for Node.js
import 'fake-indexeddb/auto';
import { WebSocket } from 'ws';
import { TextEncoder, TextDecoder } from 'node:util';

global.WebSocket = WebSocket;
global.TextEncoder = TextEncoder;
global.TextDecoder = TextDecoder;

import { JSCryptoImplementation } from '@julesl23/s5js/dist/src/api/crypto/js.js';
import { generatePhrase } from '@julesl23/s5js/dist/src/identity/seed_phrase/seed_phrase.js';

try {
  const crypto = new JSCryptoImplementation();
  const seedPhrase = generatePhrase(crypto);

  console.log('Generated S5.js seed phrase (15 words):');
  console.log(seedPhrase);
  console.log('');
  console.log('Word count:', seedPhrase.split(' ').length);
  console.log('');
  console.log('Add this to your .env file:');
  console.log(`S5_SEED_PHRASE=${seedPhrase}`);

  process.exit(0);
} catch (error) {
  console.error('Error generating seed phrase:', error);
  process.exit(1);
}

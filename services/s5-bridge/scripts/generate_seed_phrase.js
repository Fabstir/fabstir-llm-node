// Generate a valid 15-word S5.js seed phrase for testing

// Polyfill browser APIs for Node.js
import 'fake-indexeddb/auto';
import { WebSocket } from 'ws';
import { TextEncoder, TextDecoder } from 'node:util';

global.WebSocket = WebSocket;
global.TextEncoder = TextEncoder;
global.TextDecoder = TextDecoder;

import { S5 } from '@julesl23/s5js';

async function generateSeedPhrase() {
  try {
    // Create a temporary S5 instance
    const s5 = await S5.create({ initialPeers: [] });

    // Generate a seed phrase
    const seedPhrase = s5.generateSeedPhrase();

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
}

generateSeedPhrase();

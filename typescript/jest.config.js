/**
 * Shared Jest configuration for the payments-sdk TypeScript monorepo.
 * Each package inherits via:
 *   module.exports = { preset: '../../jest.config.js', roots: ['<rootDir>/src'] };
 */
module.exports = {
  testEnvironment: 'node',
  transform: {
    '^.+\\.tsx?$': ['ts-jest', { isolatedModules: true }],
  },
  moduleFileExtensions: ['ts', 'tsx', 'js', 'jsx', 'json'],
  testMatch: ['**/__tests__/**/*.(test|spec).(ts|tsx|js)'],
};

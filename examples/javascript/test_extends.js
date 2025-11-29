import { Light } from './Light.js';

class PointLight extends Light {
  constructor() {
    super();
  }
}

class AmbientLight extends Light {
  constructor() {
    super();
  }
}

export { PointLight, AmbientLight };

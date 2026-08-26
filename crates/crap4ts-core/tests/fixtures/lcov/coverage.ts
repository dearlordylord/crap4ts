function full() {
  return 1;
}

function partial(flag: boolean) {
  if (flag) {
    return 2;
  }
  return 3;
}

function zero() {
  return 4;
}

function missing() {
  return 5;
}

function outer() {
  const child = () => {
    return 6;
  };
  return child();
}

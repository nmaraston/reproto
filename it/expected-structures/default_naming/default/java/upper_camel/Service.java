package upper_camel;

import java.util.concurrent.CompletableFuture;

public interface Service {
  CompletableFuture<Void> fooBar();
}

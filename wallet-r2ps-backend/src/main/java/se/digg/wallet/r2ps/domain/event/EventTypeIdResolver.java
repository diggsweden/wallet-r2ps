package se.digg.wallet.r2ps.domain.event;

import com.fasterxml.jackson.annotation.JsonTypeInfo;
import com.fasterxml.jackson.databind.DatabindContext;
import com.fasterxml.jackson.databind.JavaType;
import com.fasterxml.jackson.databind.jsontype.impl.TypeIdResolverBase;

import java.io.IOException;

public class EventTypeIdResolver extends TypeIdResolverBase {

  private JavaType baseType;

  @Override
  public void init(JavaType baseType) {
    this.baseType = baseType;
  }

  @Override
  public String idFromValue(Object value) {
    return idFromValueAndType(value, value.getClass());
  }

  @Override
  public String idFromValueAndType(Object value, Class<?> suggestedType) {
    if (value instanceof Event event) {
      String eventType = event.metadata().eventType();
      return eventType;
    }
    return null;
  }

  @Override
  public JavaType typeFromId(DatabindContext context, String id) throws IOException {
    return switch (id) {
      case "ServerWalletRegistered" ->
          context.constructType(ServerWalletRegistered.class);
      // Add other event types here
      default ->
          throw new IllegalArgumentException("Unknown event type: " + id);
    };
  }

  @Override
  public JsonTypeInfo.Id getMechanism() {
    return JsonTypeInfo.Id.CUSTOM;
  }
}
